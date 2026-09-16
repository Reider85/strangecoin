use crate::consensus;
use crate::serialize;
use std::path::Path;

pub fn print_genesis_hash() {
    // Try current dir first, then exe dir
    let genesis_path = if Path::new("genesis.json").exists() {
        std::path::PathBuf::from("genesis.json")
    } else {
        let exe_path =
            std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path
            .parent()
            .expect("Не удалось получить директорию исполняемого файла");
        exe_dir.join("genesis.json")
    };
    let block =
        consensus::load_genesis(genesis_path.to_str().unwrap()).expect("Failed to load genesis");
    let hash = serialize::block_hash(&block);
    println!("0x{}", hex::encode(hash));
}
