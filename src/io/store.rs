use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

pub trait Store: std::fmt::Debug + Send {
    fn get(&mut self, begin: usize, length: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>>;
    fn set(&mut self, begin: usize, data: &[u8]) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Debug)]
pub struct NullStore;

impl Store for NullStore {
    fn get(
        &mut self,
        _begin: usize,
        _length: usize,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        unimplemented!("NullStore cannot be read");
    }

    fn set(&mut self, _begin: usize, _data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct MemoryStore {
    data: Vec<u8>,
}

impl MemoryStore {
    pub fn new(size: usize) -> MemoryStore {
        MemoryStore {
            data: vec![0; size],
        }
    }
}

impl Store for MemoryStore {
    fn get(&mut self, i: usize, size: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        Ok(self.data[i..(i + size)].to_vec())
    }

    fn set(&mut self, begin: usize, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.data[begin..(data.len() + begin)].copy_from_slice(data);

        Ok(())
    }
}

#[derive(Debug)]
pub struct SingleFileStore {
    file: File,
}

impl SingleFileStore {
    pub fn new(file: File, size: usize) -> Result<SingleFileStore, Box<dyn std::error::Error>> {
        file.set_len(size as u64)?;

        Ok(SingleFileStore { file })
    }
}

impl Store for SingleFileStore {
    fn get(&mut self, begin: usize, size: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        self.file.seek(SeekFrom::Start(begin as u64))?;

        let mut buf = vec![0; size];
        self.file.read_exact(&mut buf)?;

        Ok(buf)
    }

    fn set(&mut self, begin: usize, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.file.seek(SeekFrom::Start(begin as u64))?;
        self.file.write_all(data)?;

        Ok(())
    }
}

// TODO: MultiFileStore (takes in MetaInfo and handles splitting the pieces and stuff)
