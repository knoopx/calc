mod app;
mod calc;
mod engine;
mod highlight;
mod palette;
mod rates;

fn main() {
    if let Err(e) = app::run() {
        eprintln!("calc: {e}");
        std::process::exit(1);
    }
}
