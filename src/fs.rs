pub mod file;
pub mod traits;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::file::HoochFile;
    use crate::runtime::{Handle, RuntimeBuilder};
    use std::fs;
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::path::PathBuf;
    use std::str::FromStr;
    use crate::fs::traits::OpenHooch;

    const RESOURCES_DIR: &str = "./tests/resources";
    const FILE_READ_NAME: &str = "./tests/resources/my_file.txt";
    const FILE_CREATE_NAME: &str = "./tests/resources/my_file_created.txt";

    fn build_runtime() -> Handle {
        RuntimeBuilder::default().build()
    }

    #[test]
    fn test_hooch_open_file_ok() {
        let runtime = build_runtime();
        // let result = File::open("./tests/resources/my_file.txt");
        // match result {
        //     Ok(mut file) => {
        //         let mut contents = String::new();
        //         file.read_to_string(&mut contents).unwrap();
        //         println!("File contents: {}", contents);
        //     }
        //     Err(error) => {
        //         eprintln!("Failed to open file: {}", error);
        //     }
        // }

        let acutal = runtime.run_blocking(async { HoochFile::open(FILE_READ_NAME).await });
        assert!(acutal.is_ok());
    }

    #[test]
    fn test_hooch_create_file() {
        if PathBuf::from_str(FILE_CREATE_NAME).unwrap().exists() {
            let _ = std::fs::remove_file(FILE_CREATE_NAME);
        }

        let runtime_handle = build_runtime();

        let _ = runtime_handle.run_blocking(async { HoochFile::create(FILE_CREATE_NAME).await });

        assert!(PathBuf::from_str(FILE_CREATE_NAME).unwrap().exists());
        // let _ = std::fs::remove_file(FILE_CREATE_NAME);
    }

    #[test]
    fn test_hooch_open_hooch_trait() {
        const TMP_FILE_NAME: &str = "open_hooch_trait.txt";
        let file_path = PathBuf::from(format!("{}/{}", RESOURCES_DIR, TMP_FILE_NAME));
        let file_path_clone = file_path.clone();
        
        if file_path.exists(){
            let _= fs::remove_file(&file_path);
        }
        
        let runtime = build_runtime();
        runtime.run_blocking(async move { 
            let mut options = OpenOptions::new();
            options.create(true).append(true);
            let _= options.open_path(&file_path).await;
        });

        assert!(file_path_clone.exists());
        // let _ = std::fs::remove_file(&file_path_clone);
    }
}
