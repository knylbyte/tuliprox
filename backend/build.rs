use vergen::{BuildBuilder, Emitter};

fn main() {
    println!("test run for gh workflow");
    if let Ok(build) = BuildBuilder::all_build() {
        if let Ok(emitter) = Emitter::default().add_instructions(&build) {
            let _ = emitter.emit();
        }
    }
}
