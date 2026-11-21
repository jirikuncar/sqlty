use crate::common::*;
use sqlty_python::types::ty::*;

#[test]
fn rarer_types() {
    let new = Arc::new(NewType {
        name: "UserId".into(),
        module: "m".into(),
        supertype: Ty::Never,
    });
    assert!(Ty::NewType(new.clone()).same(&Ty::NewType(new)));
    assert!(Ty::union([]).is_none());
    assert_eq!(
        Ty::literal([Lit::Bool(false)]).repr(),
        "typing.Literal[False]"
    );
    assert_eq!(Ty::Never.admits_none(), None);
}
