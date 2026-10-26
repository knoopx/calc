use std::path::Path;

const FONTS: [&str; 5] = [
    "Inter-Regular.ttf",
    "JetBrainsMonoNerdFont-Regular.ttf",
    "JetBrainsMonoNerdFont-Bold.ttf",
    "NotoSans-Currency.ttf",
    "NotoEmoji-Regular.ttf",
];

fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    for name in FONTS {
        let src = Path::new("assets/fonts").join(name);
        println!("cargo::rerun-if-changed={}", src.display());
        let bytes = std::fs::read(&src).expect("bundled font");
        let packed = miniz_oxide::deflate::compress_to_vec(&bytes, 10);
        std::fs::write(Path::new(&out).join(format!("{name}.deflate")), packed).unwrap();
    }
}
