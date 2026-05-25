fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if project_index::run_cli(&args)? {
        Ok(())
    } else {
        project_index::start()
    }
}
