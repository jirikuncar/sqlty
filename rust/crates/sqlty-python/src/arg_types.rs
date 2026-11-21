//! The types of the values a module passes to its queries. [`ArgTypes`] is the one seam
//! between the checks that read arguments and the resolver behind them: driver calls,
//! helper calls and t-strings ask it, and [`SourceTypes`] answers from the module's
//! [`Scan`] and the names [`ModuleTypes`] resolves.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ruff_python_ast::{self as ast, Expr};

use crate::frozen::Table;
use crate::index::HelperIndex;
use crate::scopes::{class_of, dotted, literal_type, Key, Node, Param, Scan, Scope, IMPORTED};
use crate::types::pydantic::Known;
use crate::types::symbols::{Index, ModuleTypes, Source};
use crate::types::ty::Ty;

/// What a module says about the types of its values.
pub trait ArgTypes {
    /// The type `expr` has in the scopes `stack`, spelled as [`class_of`] spells a class
    /// (`int`, `uuid.UUID`, `list[int]`, `None`); `None` when the source does not say.
    fn arg_type(&self, expr: &Expr, stack: &[Scope]) -> Option<String>;

    /// The class of `self`, or of a variable or `self` attribute with an evident type.
    fn receiver_class(&self, receiver: &Expr, stack: &[Scope]) -> Option<String>;

    /// The arguments a tuple or list literal, or a dict literal of string keys, passes.
    fn literal_params(&self, arg: &Expr, stack: &[Scope]) -> Option<Vec<Param>> {
        let items = match arg {
            Expr::Tuple(tuple) => &tuple.elts,
            Expr::List(list) => &list.elts,
            Expr::Dict(dict) => {
                let mut found = Vec::new();
                for item in &dict.items {
                    let Some(Expr::StringLiteral(key)) = &item.key else {
                        return None;
                    };
                    found.push(Param {
                        key: Key::Name(key.value.to_str().to_string()),
                        kind: self.arg_type(&item.value, stack),
                    });
                }
                return Some(found);
            }
            _ => return None,
        };
        if items.iter().any(|item| matches!(item, Expr::Starred(_))) {
            return None;
        }
        Some(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| Param {
                    key: Key::Index(i + 1),
                    kind: self.arg_type(item, stack),
                })
                .collect(),
        )
    }
}

/// The checked modules a module imports from.
#[derive(Clone, Copy)]
pub struct Project<'s> {
    /// The helpers and modules of the project.
    pub helpers: &'s HelperIndex,
    /// The name of the module, when its source has none of its own.
    pub module: Option<&'s str>,
}

/// [`ArgTypes`] read from a module's source: what its scopes bind, and the classes, aliases
/// and `NewType`s its annotations name, its own or those of the checked modules.
pub struct SourceTypes<'a, 's> {
    source: Option<&'s Source>,
    imported: &'s Table,
    scan: &'s Scan<'a>,
    project: Option<Project<'s>>,
    index: Option<Arc<dyn Index>>,
    names: OnceCell<Option<ModuleTypes<&'s Source>>>,
    classes: RefCell<HashMap<String, Option<String>>>,
}

impl<'a, 's> SourceTypes<'a, 's> {
    /// The types of `source`, walked as `scan`. Without its [`Source`], only builtin and
    /// imported classes are known; without its project, no name another module defines.
    pub fn new(
        source: Option<&'s Source>,
        imported: &'s Table,
        scan: &'s Scan<'a>,
        project: Option<Project<'s>>,
    ) -> Self {
        SourceTypes {
            source,
            imported,
            scan,
            project,
            index: project.map(|project| project.helpers.types()),
            names: OnceCell::new(),
            classes: RefCell::default(),
        }
    }

    /// The type of the constant `dotted` imports from a checked module, unless `head` is
    /// bound in `stack`.
    fn imported_constant(&self, dotted: &str, head: &str, stack: &[Scope]) -> Option<String> {
        let project = self.project?;
        if self.binds(stack, head) {
            return None;
        }
        let (source, value) = project
            .helpers
            .fragment(self.source?, dotted, project.module)?;
        let kind = match value {
            Expr::Call(call) => self::dotted(&call.func)?,
            Expr::Name(name) => module_kinds(source).get(name.id.as_str())?.clone()?,
            value => {
                return SourceTypes::new(None, &source.imports, &Scan::new(&[]), None)
                    .arg_type(value, &[]);
            }
        };
        class_of(Some(kind.clone()), &source.imports).or_else(|| {
            let names = ModuleTypes::new(source, self.index.as_ref(), source.module.as_deref());
            spell(&resolve(&names, &kind)?)
        })
    }

    /// Whether `name` is a parameter of, or assigned in, the module or a function of `stack`.
    fn binds(&self, stack: &[Scope], name: &str) -> bool {
        stack
            .iter()
            .filter(|&&scope| !self.scan.is_class(scope))
            .chain(std::iter::once(&0))
            .any(|&scope| self.scan.types.contains_key(&(scope, name)))
    }

    /// The class a variable holds in `stack`.
    fn type_of(&self, stack: &[Scope], name: &str) -> Option<String> {
        stack
            .iter()
            .rev()
            .filter(|&&scope| !self.scan.is_class(scope))
            .chain(std::iter::once(&0))
            .find_map(|&scope| self.scan.types.get(&(scope, name)))
            .cloned()
            .flatten()
    }

    /// The class `self.name` holds in the innermost class of `stack`.
    fn attribute_type(&self, stack: &[Scope], name: &str) -> Option<String> {
        let class = stack
            .iter()
            .rev()
            .find(|&&scope| self.scan.is_class(scope))?;
        self.scan.attributes.get(&(*class, name)).cloned().flatten()
    }

    /// The class of an instance of the type spelled `name` in `stack`; a `NewType` or alias
    /// has that of the type it stands for. A name the module binds more than once, or a
    /// scope of `stack` binds, is unknown.
    fn class(&self, name: Option<String>, stack: &[Scope]) -> Option<String> {
        let name = name?;
        let head = name.split('.').next().unwrap_or_default();
        if self.source.is_some_and(|source| source.bound(head) > 1)
            || stack
                .iter()
                .any(|&scope| self.scan.types.contains_key(&(scope, head)))
        {
            return None;
        }
        if let Some(found) = class_of(Some(name.clone()), self.imported) {
            return Some(found);
        }
        if let Some(found) = self.classes.borrow().get(&name) {
            return found.clone();
        }
        let names = self.names.get_or_init(|| {
            let source = self.source?;
            let module = source.module.as_deref();
            let module = module.or(self.project.and_then(|project| project.module));
            Some(ModuleTypes::new(source, self.index.as_ref(), module))
        });
        let found = names
            .as_ref()
            .and_then(|names| spell(&resolve(names, &name)?));
        self.classes.borrow_mut().insert(name, found.clone());
        found
    }
}

impl ArgTypes for SourceTypes<'_, '_> {
    fn arg_type(&self, expr: &Expr, stack: &[Scope]) -> Option<String> {
        let name = match expr {
            Expr::NoneLiteral(_) => "None",
            Expr::List(ast::ExprList { elts: items, .. })
            | Expr::Tuple(ast::ExprTuple { elts: items, .. })
            | Expr::Set(ast::ExprSet { elts: items, .. }) => {
                let types: HashSet<Option<String>> = items
                    .iter()
                    .map(|item| match item {
                        Expr::Starred(_) => None,
                        _ => self.arg_type(item, stack),
                    })
                    .collect();
                if let [Some(only)] = types.iter().collect::<Vec<_>>().as_slice() {
                    if only != "None" {
                        return Some(format!("list[{only}]"));
                    }
                }
                "list"
            }
            Expr::Name(name) => {
                let name = name.id.as_str();
                return self
                    .class(self.type_of(stack, name), stack)
                    .or_else(|| self.imported_constant(name, name, stack));
            }
            Expr::Attribute(_) => {
                let dotted = dotted(expr)?;
                let head = dotted.split('.').next().unwrap_or_default();
                return self.imported_constant(&dotted, head, stack);
            }
            Expr::Call(call) => return self.class(dotted(&call.func), stack),
            expr => literal_type(expr)?,
        };
        Some(name.to_string())
    }

    fn receiver_class(&self, receiver: &Expr, stack: &[Scope]) -> Option<String> {
        let base = match receiver {
            Expr::Attribute(attr) => attr.value.as_ref(),
            expr => expr,
        };
        let Expr::Name(base_name) = base else {
            return None;
        };
        for pair in stack.windows(2).rev() {
            let (Node::Class(class), Node::Function(method)) =
                (self.scan.scopes[pair[0]], self.scan.scopes[pair[1]])
            else {
                continue;
            };
            let is_self = method
                .parameters
                .args
                .first()
                .is_some_and(|first| first.parameter.name.as_str() == base_name.id.as_str());
            let is_static = method
                .decorator_list
                .iter()
                .any(|d| dotted(&d.expression).as_deref() == Some("staticmethod"));
            if is_self && !is_static {
                return match receiver {
                    Expr::Attribute(attr) => self.attribute_type(stack, attr.attr.as_str()),
                    _ => Some(class.name.to_string()),
                };
            }
        }
        match receiver {
            Expr::Name(name) => self.type_of(stack, name.id.as_str()),
            _ => None,
        }
    }
}

/// The type `names` gives the dotted `name`.
fn resolve<S: std::ops::Deref<Target = Source>>(names: &ModuleTypes<S>, name: &str) -> Option<Ty> {
    let parsed = crate::parse::expression(name)?;
    names
        .source
        .resolver(Known::Imported)
        .resolve(&parsed.syntax().body, name, names)
}

/// `ty` spelled as [`class_of`] spells a class: a `NewType` as its supertype, nothing for
/// what is not a builtin or a known imported class.
fn spell(ty: &Ty) -> Option<String> {
    match ty {
        Ty::NewType(newtype) => spell(&newtype.supertype),
        Ty::Class(class) if class.module == "builtins" => {
            class_of(Some(class.name.clone()), &Table::default())
        }
        Ty::Class(class) => {
            let full = format!("{}.{}", class.module, class.name);
            IMPORTED.contains(&full.as_str()).then_some(full)
        }
        _ => None,
    }
}

/// The class each module-level name of `source` holds, as [`SourceTypes`] reads a variable,
/// walked once per module.
fn module_kinds(source: &Source) -> &HashMap<String, Option<String>> {
    source.kinds.get_or_init(|| {
        Scan::new(source.body())
            .types
            .into_iter()
            .filter(|((scope, _), _)| *scope == 0)
            .map(|((_, name), kind)| (name.to_string(), kind))
            .collect()
    })
}
