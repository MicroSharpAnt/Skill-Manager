fn main() {
    let path = std::env::args().nth(1).expect("项目路径");
    match skill_manager::engine::Repo::open(std::path::Path::new(&path)).and_then(|r| r.scan()) {
        Ok(s) => println!("{}", serde_json::to_string_pretty(&s).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
