use crate::fs::file::HoochFile;
use std::io;
use std::path::Path;

pub trait OpenHooch {
    fn open_path(self, path: &Path) -> impl Future<Output = Result<HoochFile, io::Error>>;
}
