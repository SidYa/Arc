fn main() {
    // LNK4104 "export of DllGetClassObject should be PRIVATE": only about import libraries, which nobody uses
    println!("cargo:rustc-cdylib-link-arg=/IGNORE:4104");
}
