use std::{env, fs, path::PathBuf};

fn main() {
    let spec_path = "../../openapi.json";
    println!("cargo:rerun-if-changed={spec_path}");

    let file = fs::File::open(spec_path).expect("open openapi.json");
    let spec: openapiv3::OpenAPI =
        serde_json::from_reader(file).expect("parse openapi.json");

    let mut generator = progenitor::Generator::default();
    let tokens = generator.generate_tokens(&spec).expect("generate client");
    let ast = syn::parse2(tokens).expect("parse generated tokens");
    let code = prettyplease::unparse(&ast);

    let mut out = PathBuf::from(env::var("OUT_DIR").unwrap());
    out.push("codegen.rs");
    fs::write(out, code).expect("write codegen.rs");
}
