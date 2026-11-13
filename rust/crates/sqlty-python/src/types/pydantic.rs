//! Pydantic's types, read without pydantic.
//!
//! Each type has two readings. A type checker sees pydantic's `TYPE_CHECKING` declarations
//! (`EmailStr` is `str`, `AnyUrl` a class of its own); a `TypedDict` or plain dataclass row
//! holds what the driver returns, so it is checked against those. A Pydantic model or
//! dataclass validates its input instead: `AnyUrl` takes a `str`, `ByteSize` an `int` or a
//! `str`.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::ty::{builtins, Class, Ty};

const URLS: &str =
    "AnyUrl AnyHttpUrl HttpUrl AnyWebsocketUrl WebsocketUrl FileUrl FtpUrl AmqpDsn ClickHouseDsn \
     CockroachDsn KafkaDsn MariaDBDsn MongoDsn MySQLDsn NatsDsn PostgresDsn RedisDsn SnowflakeDsn";

/// `_IMPORTED_TYPES` and `pydantic_names`: the types qualified names resolve to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Known {
    /// The `decimal`, `datetime` and `uuid` types and Pydantic's `Strict*` aliases.
    Imported,
    /// Those, and Pydantic's types as a type checker reads them.
    Checker,
    /// Those, and Pydantic's types by the input a model takes.
    Model,
}

impl Known {
    /// The type of `key`: `module.Name`, `module.name()` for a call, `module.Name[]` for a
    /// generic.
    pub fn get(self, key: &str) -> Option<&'static Ty> {
        static TABLES: LazyLock<[HashMap<String, Ty>; 3]> = LazyLock::new(tables);
        TABLES[self as usize].get(key)
    }
}

fn union(members: &[&Ty]) -> Ty {
    Ty::union(members.iter().map(|m| (*m).clone())).unwrap_or(Ty::Never)
}

#[allow(clippy::too_many_lines)]
fn tables() -> [HashMap<String, Ty>; 3] {
    let b = builtins();
    let class = |c: &std::sync::Arc<Class>| Ty::Class(c.clone());
    let stand_in = |name: &str, bases: &[std::sync::Arc<Class>]| {
        Ty::Class(Class::new(name, "pydantic", bases).expect("bases Python can combine"))
    };
    let (int, float, str, bytes) = (
        class(&b.int),
        class(&b.float),
        class(&b.str),
        class(&b.bytes),
    );
    let (date, datetime, uuid) = (class(&b.date), class(&b.datetime), class(&b.uuid));
    let text = union(&[&str, &bytes]);
    let any = Ty::any();
    let json = union(&[
        &Ty::Generic(b.list.clone(), vec![any.clone()]),
        &Ty::Generic(b.dict.clone(), vec![str.clone(), any]),
        &str,
        &class(&b.bool),
        &int,
        &float,
        &Ty::none(),
    ]);
    let address = union(&[&class(&b.ipv4_address), &class(&b.ipv6_address)]);
    let network = union(&[&class(&b.ipv4_network), &class(&b.ipv6_network)]);
    let interface = union(&[&class(&b.ipv4_interface), &class(&b.ipv6_interface)]);

    // name -> (type for a type checker, input of a model)
    let mut types: Vec<(String, Ty, Ty)> = Vec::new();
    let mut networks: Vec<(String, Ty, Ty)> = Vec::new();
    let mut same = |names: &[&str], tp: &Ty| {
        for name in names {
            types.push(((*name).to_string(), tp.clone(), tp.clone()));
        }
    };
    same(
        &[
            "PositiveInt",
            "NegativeInt",
            "NonNegativeInt",
            "NonPositiveInt",
        ],
        &int,
    );
    same(
        &[
            "PositiveFloat",
            "NegativeFloat",
            "NonNegativeFloat",
            "NonPositiveFloat",
            "FiniteFloat",
        ],
        &float,
    );
    same(
        &[
            "AwareDatetime",
            "NaiveDatetime",
            "PastDatetime",
            "FutureDatetime",
        ],
        &datetime,
    );
    same(&["PastDate", "FutureDate"], &date);
    same(
        &[
            "UUID1", "UUID3", "UUID4", "UUID5", "UUID6", "UUID7", "UUID8",
        ],
        &uuid,
    );
    same(&["JsonValue"], &json);
    for (name, checker) in [
        ("Base64Str", &str),
        ("Base64UrlStr", &str),
        ("Base64Bytes", &bytes),
        ("Base64UrlBytes", &bytes),
    ] {
        types.push((name.to_string(), checker.clone(), text.clone()));
    }
    types.push((
        "ByteSize".to_string(),
        stand_in("ByteSize", std::slice::from_ref(&b.int)),
        union(&[&int, &str]),
    ));
    types.push((
        "PaymentCardNumber".to_string(),
        stand_in("PaymentCardNumber", std::slice::from_ref(&b.str)),
        str.clone(),
    ));
    types.push((
        "SecretStr".to_string(),
        stand_in("SecretStr", &[]),
        text.clone(),
    ));
    types.push((
        "SecretBytes".to_string(),
        stand_in("SecretBytes", &[]),
        text.clone(),
    ));

    for name in URLS.split_whitespace() {
        networks.push((name.to_string(), stand_in(name, &[]), str.clone()));
    }
    networks.push(("EmailStr".to_string(), str.clone(), str.clone()));
    networks.push((
        "NameEmail".to_string(),
        stand_in("NameEmail", &[]),
        str.clone(),
    ));
    networks.push((
        "IPvAnyAddress".to_string(),
        address.clone(),
        union(&[&address, &str]),
    ));
    networks.push((
        "IPvAnyNetwork".to_string(),
        network.clone(),
        union(&[&network, &address, &str]),
    ));
    networks.push((
        "IPvAnyInterface".to_string(),
        interface.clone(),
        union(&[&interface, &network, &address, &str]),
    ));

    let mut imported: HashMap<String, Ty> = HashMap::new();
    for (name, tp) in [
        ("decimal.Decimal", class(&b.decimal)),
        ("datetime.date", date.clone()),
        ("datetime.datetime", datetime.clone()),
        ("datetime.time", class(&b.time)),
        ("datetime.timedelta", class(&b.timedelta)),
        ("uuid.UUID", uuid.clone()),
    ] {
        imported.insert(name.to_string(), tp);
    }
    for (name, tp) in [
        ("StrictInt", &int),
        ("StrictFloat", &float),
        ("StrictBool", &class(&b.bool)),
        ("StrictStr", &str),
        ("StrictBytes", &bytes),
    ] {
        imported.insert(format!("pydantic.{name}"), tp.clone());
        imported.insert(format!("pydantic.types.{name}"), tp.clone());
    }

    let mut checker = imported.clone();
    let mut model = imported.clone();
    for (module, table) in [("types", &types), ("networks", &networks)] {
        for (name, for_checker, for_model) in table {
            for key in [
                format!("pydantic.{name}"),
                format!("pydantic.{module}.{name}"),
            ] {
                checker.insert(key.clone(), for_checker.clone());
                model.insert(key, for_model.clone());
            }
        }
    }
    // `conint(...)` and the like, by the type of their result.
    for (name, tp) in [
        ("conint", &int),
        ("confloat", &float),
        ("condecimal", &class(&b.decimal)),
        ("constr", &str),
        ("conbytes", &bytes),
        ("condate", &date),
        ("conlist", &class(&b.list)),
    ] {
        for key in [
            format!("pydantic.{name}()"),
            format!("pydantic.types.{name}()"),
        ] {
            checker.insert(key.clone(), tp.clone());
            model.insert(key, tp.clone());
        }
    }
    // A model's input of `Json[T]` is the JSON text, whatever `T`.
    for key in ["pydantic.Json", "pydantic.types.Json"] {
        model.insert(key.to_string(), text.clone());
        model.insert(format!("{key}[]"), text.clone());
    }
    [imported, checker, model]
}

/// Whether `qualname` is one of Pydantic's `Strict*` aliases.
pub fn is_strict_name(qualname: &str) -> bool {
    qualname
        .strip_prefix("pydantic.types.")
        .or_else(|| qualname.strip_prefix("pydantic."))
        .is_some_and(|name| {
            [
                "StrictInt",
                "StrictFloat",
                "StrictBool",
                "StrictStr",
                "StrictBytes",
            ]
            .contains(&name)
        })
}
