//! Resolved types: the `typing` objects the Python passes build, as values.
//!
//! A [`Class`] is nominal, with the method resolution order and instance layout CPython
//! gives it, so a stand-in built from bases Python cannot combine is unknown here too.
//! Equality ([`Ty::same`]) is that of the `typing` objects: classes and `NewType`s by
//! identity, unions and literals as sets.

use std::sync::{Arc, LazyLock};

/// What decides whether two bases can be combined: the ancestor that adds instance state.
enum Layout {
    Object,
    Own,
    Of(Arc<Class>),
}

pub struct Class {
    pub name: String,
    pub module: String,
    /// The method resolution order after the class itself, `object` included.
    mro: Vec<Arc<Class>>,
    layout: Layout,
    subclassable: bool,
}

impl std::fmt::Debug for Class {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.module, self.name)
    }
}

impl Class {
    /// `type(name, bases, {"__module__": module})`; `None` where that raises `TypeError`.
    pub fn new(name: &str, module: &str, bases: &[Arc<Class>]) -> Option<Arc<Class>> {
        let object = [builtins().object.clone()];
        let bases = if bases.is_empty() { &object } else { bases };
        let mut winner: Option<Arc<Class>> = None;
        for (index, base) in bases.iter().enumerate() {
            if !base.subclassable || bases[..index].iter().any(|b| Arc::ptr_eq(b, base)) {
                return None;
            }
            let Some(candidate) = solid(base) else {
                continue;
            };
            match &winner {
                None => winner = Some(candidate),
                Some(current) if is_subclass(current, &candidate) => {}
                Some(current) if is_subclass(&candidate, current) => winner = Some(candidate),
                Some(_) => return None,
            }
        }
        Some(Arc::new(Class {
            name: name.to_string(),
            module: module.to_string(),
            mro: linearize(bases)?,
            layout: winner.map_or(Layout::Object, Layout::Of),
            subclassable: true,
        }))
    }

    fn real(name: &str, module: &str, own: bool, bases: &[&Arc<Class>]) -> Arc<Class> {
        let bases: Vec<Arc<Class>> = bases.iter().map(|b| Arc::clone(b)).collect();
        let layout = match bases.iter().find_map(solid) {
            _ if own => Layout::Own,
            Some(base) => Layout::Of(base),
            None => Layout::Object,
        };
        Arc::new(Class {
            name: name.to_string(),
            module: module.to_string(),
            mro: linearize(&bases).unwrap_or_default(),
            layout,
            subclassable: true,
        })
    }
}

fn solid(class: &Arc<Class>) -> Option<Arc<Class>> {
    match &class.layout {
        Layout::Object => None,
        Layout::Own => Some(class.clone()),
        Layout::Of(base) => Some(base.clone()),
    }
}

/// The C3 linearization of a class with `bases`, the class itself left out.
fn linearize(bases: &[Arc<Class>]) -> Option<Vec<Arc<Class>>> {
    let mut sequences: Vec<Vec<Arc<Class>>> = bases
        .iter()
        .map(|base| {
            let mut sequence = vec![base.clone()];
            sequence.extend(base.mro.iter().cloned());
            sequence
        })
        .collect();
    sequences.push(bases.to_vec());
    let mut out = Vec::new();
    loop {
        sequences.retain(|s| !s.is_empty());
        if sequences.is_empty() {
            return Some(out);
        }
        let head = sequences.iter().map(|s| &s[0]).find(|candidate| {
            !sequences
                .iter()
                .any(|s| s[1..].iter().any(|c| Arc::ptr_eq(c, candidate)))
        })?;
        let head = head.clone();
        for sequence in &mut sequences {
            if Arc::ptr_eq(&sequence[0], &head) {
                sequence.remove(0);
            }
        }
        out.push(head);
    }
}

/// `issubclass(class, other)`.
pub fn is_subclass(class: &Arc<Class>, other: &Arc<Class>) -> bool {
    Arc::ptr_eq(class, other) || class.mro.iter().any(|c| Arc::ptr_eq(c, other))
}

/// The classes `sqlty` reads by name: builtins, column types and `typing.Any`.
pub struct Builtins {
    pub object: Arc<Class>,
    pub any: Arc<Class>,
    pub none: Arc<Class>,
    pub int: Arc<Class>,
    pub float: Arc<Class>,
    pub complex: Arc<Class>,
    pub str: Arc<Class>,
    pub bytes: Arc<Class>,
    pub bool: Arc<Class>,
    pub list: Arc<Class>,
    pub dict: Arc<Class>,
    pub set: Arc<Class>,
    pub frozenset: Arc<Class>,
    pub tuple: Arc<Class>,
    pub date: Arc<Class>,
    pub datetime: Arc<Class>,
    pub time: Arc<Class>,
    pub timedelta: Arc<Class>,
    pub uuid: Arc<Class>,
    pub decimal: Arc<Class>,
    pub ipv4_address: Arc<Class>,
    pub ipv6_address: Arc<Class>,
    pub ipv4_interface: Arc<Class>,
    pub ipv6_interface: Arc<Class>,
    pub ipv4_network: Arc<Class>,
    pub ipv6_network: Arc<Class>,
}

pub fn builtins() -> &'static Builtins {
    static BUILTINS: LazyLock<Builtins> = LazyLock::new(|| {
        let object = Arc::new(Class {
            name: "object".to_string(),
            module: "builtins".to_string(),
            mro: Vec::new(),
            layout: Layout::Object,
            subclassable: true,
        });
        let own = |name: &str, module: &str| Class::real(name, module, true, &[&object]);
        let sealed = |name: &str, bases: &[Arc<Class>]| {
            let mut mro = bases.to_vec();
            mro.push(object.clone());
            Arc::new(Class {
                name: name.to_string(),
                module: "builtins".to_string(),
                mro,
                layout: Layout::Own,
                subclassable: false,
            })
        };
        let int = own("int", "builtins");
        let date = own("date", "datetime");
        let ip = "ipaddress";
        let base = Class::real("_IPAddressBase", ip, false, &[&object]);
        let address = Class::real("_BaseAddress", ip, false, &[&base]);
        let network = Class::real("_BaseNetwork", ip, false, &[&base]);
        let v4 = Class::real("_BaseV4", ip, false, &[&object]);
        let v6 = Class::real("_BaseV6", ip, false, &[&object]);
        let ipv4_address = Class::real("IPv4Address", ip, true, &[&v4, &address]);
        let ipv6_address = Class::real("IPv6Address", ip, true, &[&v6, &address]);
        Builtins {
            any: Class::real("Any", "typing", false, &[&object]),
            none: sealed("NoneType", &[]),
            float: own("float", "builtins"),
            complex: own("complex", "builtins"),
            str: own("str", "builtins"),
            bytes: own("bytes", "builtins"),
            bool: sealed("bool", std::slice::from_ref(&int)),
            list: own("list", "builtins"),
            dict: own("dict", "builtins"),
            set: own("set", "builtins"),
            frozenset: own("frozenset", "builtins"),
            tuple: own("tuple", "builtins"),
            datetime: Class::real("datetime", "datetime", true, &[&date]),
            time: own("time", "datetime"),
            timedelta: own("timedelta", "datetime"),
            uuid: own("UUID", "uuid"),
            decimal: own("Decimal", "decimal"),
            ipv4_interface: Class::real("IPv4Interface", ip, false, &[&ipv4_address]),
            ipv6_interface: Class::real("IPv6Interface", ip, false, &[&ipv6_address]),
            ipv4_network: Class::real("IPv4Network", ip, false, &[&v4, &network]),
            ipv6_network: Class::real("IPv6Network", ip, false, &[&v6, &network]),
            ipv4_address,
            ipv6_address,
            int,
            date,
            object,
        }
    });
    &BUILTINS
}

impl Builtins {
    /// `PYTHON_TYPE_MAP[name]`: the class of an inferred column type.
    pub fn column(&self, name: &str) -> Option<&Arc<Class>> {
        Some(match name {
            "int" => &self.int,
            "str" => &self.str,
            "float" => &self.float,
            "bool" => &self.bool,
            "bytes" => &self.bytes,
            "list" => &self.list,
            "dict" => &self.dict,
            "datetime.date" => &self.date,
            "datetime.datetime" => &self.datetime,
            "datetime.time" => &self.time,
            "datetime.timedelta" => &self.timedelta,
            "uuid.UUID" => &self.uuid,
            "decimal.Decimal" => &self.decimal,
            "typing.Any" => &self.any,
            "None" => &self.none,
            "ipaddress.IPv4Address" => &self.ipv4_address,
            "ipaddress.IPv6Address" => &self.ipv6_address,
            "ipaddress.IPv4Interface" => &self.ipv4_interface,
            "ipaddress.IPv6Interface" => &self.ipv6_interface,
            "ipaddress.IPv4Network" => &self.ipv4_network,
            "ipaddress.IPv6Network" => &self.ipv6_network,
            _ => return None,
        })
    }
}

pub struct NewType {
    pub name: String,
    pub module: String,
    pub supertype: Ty,
}

/// A value of `Literal[...]`; an integer as its decimal digits, of any size.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lit {
    Str(String),
    Int(String),
    Bool(bool),
}

#[derive(Clone)]
pub enum Ty {
    Never,
    Class(Arc<Class>),
    /// `list[int]`: one of `list`, `dict`, `set`, `frozenset`, `tuple` with arguments.
    Generic(Arc<Class>, Vec<Ty>),
    Literal(Vec<Lit>),
    /// Two members or more, none of them a union, no two the same.
    Union(Vec<Ty>),
    NewType(Arc<NewType>),
}

impl std::fmt::Debug for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.repr())
    }
}

impl Ty {
    pub fn any() -> Ty {
        Ty::Class(builtins().any.clone())
    }

    pub fn none() -> Ty {
        Ty::Class(builtins().none.clone())
    }

    pub fn is(&self, class: &Arc<Class>) -> bool {
        matches!(self, Ty::Class(c) if Arc::ptr_eq(c, class))
    }

    pub fn is_any(&self) -> bool {
        self.is(&builtins().any)
    }

    pub fn is_none(&self) -> bool {
        self.is(&builtins().none)
    }

    fn is_top(&self) -> bool {
        self.is_any() || self.is(&builtins().object)
    }

    /// `self == other` of the `typing` objects.
    pub fn same(&self, other: &Ty) -> bool {
        match (self, other) {
            (Ty::Never, Ty::Never) => true,
            (Ty::Class(a), Ty::Class(b)) => Arc::ptr_eq(a, b),
            (Ty::Generic(a, x), Ty::Generic(b, y)) => {
                Arc::ptr_eq(a, b) && x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.same(q))
            }
            (Ty::Literal(a), Ty::Literal(b)) => {
                a.iter().all(|v| b.contains(v)) && b.iter().all(|v| a.contains(v))
            }
            (Ty::Union(a), Ty::Union(b)) => {
                a.iter().all(|m| b.iter().any(|n| m.same(n)))
                    && b.iter().all(|m| a.iter().any(|n| m.same(n)))
            }
            (Ty::NewType(a), Ty::NewType(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// `Literal[values]`, a repeated value kept once.
    pub fn literal(values: impl IntoIterator<Item = Lit>) -> Ty {
        let mut kept: Vec<Lit> = Vec::new();
        for value in values {
            if !kept.contains(&value) {
                kept.push(value);
            }
        }
        Ty::Literal(kept)
    }

    /// `functools.reduce(operator.or_, members)`; `None` for no members.
    pub fn union(members: impl IntoIterator<Item = Ty>) -> Option<Ty> {
        let mut kept: Vec<Ty> = Vec::new();
        for member in members {
            for part in member.members() {
                if !kept.iter().any(|k| k.same(part)) {
                    kept.push(part.clone());
                }
            }
        }
        match kept.len() {
            0 => None,
            1 => kept.pop(),
            _ => Some(Ty::Union(kept)),
        }
    }

    /// `_union_members`.
    pub fn members(&self) -> &[Ty] {
        match self {
            Ty::Union(members) => members,
            other => std::slice::from_ref(other),
        }
    }

    /// `isinstance(tp, type) and not typing.get_args(tp)`.
    fn plain(&self) -> Option<&Arc<Class>> {
        match self {
            Ty::Class(class) => Some(class),
            _ => None,
        }
    }

    /// `repr(tp)`.
    pub fn repr(&self) -> String {
        match self {
            Ty::Class(class) if class.module == "typing" => format!("typing.{}", class.name),
            Ty::Class(class) => format!("<class '{}'>", qualified(class)),
            _ => self.item(),
        }
    }

    /// The spelling of the type inside the `repr` of another.
    fn item(&self) -> String {
        match self {
            Ty::Never => "typing.Never".to_string(),
            Ty::Class(class) if Arc::ptr_eq(class, &builtins().none) => "None".to_string(),
            Ty::Class(class) => qualified(class),
            Ty::Generic(origin, args) if args.is_empty() => format!("{}[()]", qualified(origin)),
            Ty::Generic(origin, args) => format!("{}[{}]", qualified(origin), items(args)),
            // `Literal[()]` holds the empty tuple.
            Ty::Literal(values) if values.is_empty() => "typing.Literal[()]".to_string(),
            Ty::Literal(values) => {
                let values: Vec<String> = values
                    .iter()
                    .map(|value| match value {
                        Lit::Str(text) => crate::unparse::repr_str(text),
                        Lit::Int(digits) => digits.clone(),
                        Lit::Bool(true) => "True".to_string(),
                        Lit::Bool(false) => "False".to_string(),
                    })
                    .collect();
                format!("typing.Literal[{}]", values.join(", "))
            }
            Ty::Union(members) => {
                let members: Vec<String> = members.iter().map(Ty::item).collect();
                members.join(" | ")
            }
            Ty::NewType(new) => format!("{}.{}", new.module, new.name),
        }
    }

    /// `_type_repr`: how a message names the type.
    pub fn display(&self) -> String {
        match self {
            Ty::Class(class) => class.name.clone(),
            _ => self.repr().replace("typing.", ""),
        }
    }

    /// `_erase`: `NewType` as its supertype and `list[str]` as `list`, as inferred column
    /// types are bare.
    #[must_use]
    pub fn erase(&self) -> Ty {
        let mut tp = self;
        while let Ty::NewType(new) = tp {
            tp = &new.supertype;
        }
        match tp {
            Ty::Union(members) => {
                Ty::union(members.iter().map(Ty::erase)).unwrap_or_else(|| tp.clone())
            }
            Ty::Generic(origin, _) => Ty::Class(origin.clone()),
            other => other.clone(),
        }
    }

    /// `admits_none`.
    pub fn admits_none(&self) -> Option<bool> {
        let verdicts: Vec<Option<bool>> = self
            .members()
            .iter()
            .map(|member| match member {
                Ty::Class(_) if member.is_none() || member.is_top() => Some(true),
                Ty::NewType(new) => new.supertype.admits_none(),
                // `None` is never a value of the `Literal`s read here.
                Ty::Class(_) | Ty::Generic(..) | Ty::Literal(_) => Some(false),
                Ty::Never | Ty::Union(_) => None,
            })
            .collect();
        if verdicts.contains(&Some(true)) {
            Some(true)
        } else if verdicts.contains(&None) {
            None
        } else {
            Some(false)
        }
    }

    /// `is_optional`: whether the annotation spells `None` out.
    pub fn is_optional(&self) -> bool {
        matches!(self, Ty::Union(members) if members.iter().any(Ty::is_none))
    }
}

fn qualified(class: &Class) -> String {
    if class.module == "builtins" {
        class.name.clone()
    } else {
        format!("{}.{}", class.module, class.name)
    }
}

fn items(args: &[Ty]) -> String {
    let args: Vec<String> = args.iter().map(Ty::item).collect();
    args.join(", ")
}

/// Which classes a value of another may fill a field of: `int` a `float` one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Promotions {
    /// `int` to `float` to `complex`.
    Numeric,
    /// Pydantic's lax mode; <https://docs.pydantic.dev/latest/concepts/conversion_table/>.
    Lax,
}

impl Promotions {
    fn from(self, target: &Arc<Class>) -> Vec<&'static Arc<Class>> {
        let b = builtins();
        let is = |class: &Arc<Class>| Arc::ptr_eq(target, class);
        if is(&b.float) {
            vec![&b.int]
        } else if is(&b.complex) {
            vec![&b.int, &b.float]
        } else if self == Promotions::Numeric {
            Vec::new()
        } else if is(&b.int) {
            vec![&b.decimal, &b.float]
        } else if is(&b.decimal) {
            vec![&b.int, &b.float]
        } else if is(&b.bool) {
            vec![&b.int]
        } else if is(&b.datetime) {
            vec![&b.date]
        } else if is(&b.tuple) || is(&b.set) || is(&b.frozenset) {
            vec![&b.list]
        } else {
            Vec::new()
        }
    }
}

/// `_is_subtype`: whether `src` is assignable to `dst`; `None` when that can't be decided,
/// as for an unknown type (`None`).
pub fn is_subtype(src: Option<&Ty>, dst: Option<&Ty>, promotions: Promotions) -> Option<bool> {
    let (src, dst) = (src?, dst?);
    if src.is_any() || dst.is_top() || matches!(src, Ty::Never) {
        return Some(true);
    }
    let targets = dst.members();
    let mut result = Some(true);
    for member in src.members() {
        match member_fits(member, targets, promotions) {
            Some(false) => return Some(false),
            None => result = None,
            Some(true) => {}
        }
    }
    result
}

fn member_fits(src: &Ty, targets: &[Ty], promotions: Promotions) -> Option<bool> {
    if src.is_any() || matches!(src, Ty::Never) {
        return Some(true);
    }
    if targets.iter().any(|d| src.same(d) || d.is_top()) {
        return Some(true);
    }
    let class = src.plain()?;
    let mut decided = true;
    for dst in targets {
        if matches!(dst, Ty::Never) {
            continue;
        }
        let Some(target) = dst.plain() else {
            decided = false;
            continue;
        };
        if is_subclass(class, target)
            || promotions
                .from(target)
                .iter()
                .any(|from| is_subclass(class, from))
        {
            return Some(true);
        }
    }
    decided.then_some(false)
}

/// `_consistent`: whether the two are equivalent; `None` if unsure.
pub fn consistent(src: Option<&Ty>, dst: Option<&Ty>) -> Option<bool> {
    if src.is_some_and(Ty::is_any) || dst.is_some_and(Ty::is_any) {
        return Some(true);
    }
    let forward = is_subtype(src, dst, Promotions::Numeric);
    let backward = is_subtype(dst, src, Promotions::Numeric);
    if forward == Some(false) || backward == Some(false) {
        return Some(false);
    }
    (forward == Some(true) && backward == Some(true)).then_some(true)
}

/// `_column_type`: the type of an inferred column type; `list[T]` is `list`, unknown `None`.
pub fn column_type(python_type: &str) -> Option<Ty> {
    let b = builtins();
    if python_type.starts_with("list[") {
        return Some(Ty::Class(b.list.clone()));
    }
    let members: Option<Vec<Ty>> = python_type
        .split(" | ")
        .map(|name| b.column(py_strip(name)).cloned().map(Ty::Class))
        .collect();
    Ty::union(members?)
}

/// `str.strip()`.
pub fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}
