use std::{env, fs, path::Path};

/// typify only recognises tagged unions and literal fields when they are single-value enums, not `const`.
fn const_to_enum(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(constant) = map.remove("const") {
                map.insert("enum".into(), serde_json::Value::Array(vec![constant]));
            }
            map.values_mut().for_each(const_to_enum);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(const_to_enum),
        _ => {}
    }
}

fn main() {
    let schema_path = Path::new("../../schemas/protocol.schema.json");
    println!("cargo:rerun-if-changed={}", schema_path.display());
    let mut schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_path).unwrap()).unwrap();
    const_to_enum(&mut schema);
    let mut space =
        typify::TypeSpace::new(typify::TypeSpaceSettings::default().with_struct_builder(false));
    space
        .add_root_schema(serde_json::from_value(schema).unwrap())
        .unwrap();
    let file = syn::parse2(space.to_stream()).unwrap();
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("protocol.rs");
    fs::write(out, prettyplease::unparse(&file)).unwrap();
}
