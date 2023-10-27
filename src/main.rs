use clap::Args;
use flate2::{read::ZlibDecoder, write::ZlibEncoder};
use sha1::{Digest, Sha1};
use std::io::prelude::*;
use std::{fs, path::PathBuf};

use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    CatFile(CatFileArgs),
    HashObject(HashObjectArgs),
}

#[derive(Debug, Args)]
struct CatFileArgs {
    #[arg(short)]
    pretty_print: bool,
    object: String,
}

#[derive(Debug, Args)]
struct HashObjectArgs {
    #[arg(short)]
    write: bool,
    path: PathBuf,
}

enum GitObject {
    Blob(Blob),
}

struct Blob {
    object_hash: String,
    length: usize,
    contents: String,
}

impl Blob {
    fn new(object_hash: String, length: usize, contents: String) -> Self {
        Self {
            object_hash,
            length,
            contents,
        }
    }

    fn from_contents(contents: &str) -> Self {
        let length = contents.len();
        let header = format!("blob {}\0", length);
        let store = format!("{}{}", header, contents);

        let mut hasher = Sha1::new();
        hasher.update(store.as_bytes());
        let result = hasher.finalize();
        let object_hash = format!("{:x}", result);

        Self {
            object_hash,
            length,
            contents: contents.to_string(),
        }
    }

    fn header(&self) -> String {
        format!("blob {}\0", self.length)
    }

    fn as_bytes(&self) -> Vec<u8> {
        format!("{}{}", self.header(), self.contents).into_bytes()
    }

    fn path(&self) -> PathBuf {
        Self::path_from_object_hash(&self.object_hash)
    }

    fn dir_path(&self) -> PathBuf {
        Self::dir_path_from_object_hash(&self.object_hash)
    }

    fn path_from_object_hash(object_hash: &str) -> PathBuf {
        let directory = object_hash.chars().take(2).collect::<String>();
        let filename = object_hash.chars().skip(2).collect::<String>();
        format!(".git/objects/{}/{}", directory, filename).into()
    }

    fn dir_path_from_object_hash(object_hash: &str) -> PathBuf {
        let directory = object_hash.chars().take(2).collect::<String>();
        format!(".git/objects/{}", directory).into()
    }
}

fn git_init() {
    fs::create_dir(".git").unwrap();
    fs::create_dir(".git/objects").unwrap();
    fs::create_dir(".git/refs").unwrap();
    fs::write(".git/HEAD", "ref: refs/heads/master\n").unwrap();
    println!("Initialized git directory")
}

fn git_cat_file(args: &CatFileArgs) {
    let path = Blob::path_from_object_hash(&args.object);

    let file = fs::read(path).unwrap();

    let mut decompressed = ZlibDecoder::new(&*file);
    let mut contents = String::new();
    decompressed.read_to_string(&mut contents).unwrap();

    let contents = contents.split('\0').collect::<Vec<&str>>();
    let (tipe, length) = contents[0].split_once(' ').unwrap();
    let length: usize = length.parse().unwrap();

    let object = match tipe {
        "blob" => GitObject::Blob(Blob::new(
            args.object.clone(),
            length,
            contents[1][0..length].to_string(),
        )),
        _ => panic!("Unknown object type: {}", tipe),
    };

    match object {
        GitObject::Blob(blob) => {
            print!("{}", blob.contents);
        }
    }
}

fn git_hash_object(args: &HashObjectArgs) {
    let contents = fs::read_to_string(&args.path).unwrap();
    let blob = Blob::from_contents(&contents);

    if args.write {
        fs::create_dir_all(blob.dir_path()).unwrap();

        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&blob.as_bytes()).unwrap();

        let compressed = encoder.finish().unwrap();
        fs::write(blob.path(), compressed).unwrap();
    }

    println!("{}", blob.object_hash);
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Init => git_init(),
        Commands::CatFile(args) => {
            git_cat_file(args);
        }
        Commands::HashObject(args) => {
            git_hash_object(args);
        }
    }
}
