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
    LsTree(LsTreeArgs),
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

#[derive(Debug, Args)]
struct LsTreeArgs {
    #[arg(long)]
    name_only: bool,
    object: String,
}

trait HasObjectHash {
    fn hash(&self) -> &str;
}

trait GitPath {
    fn path(&self) -> PathBuf;
    fn dir_path(&self) -> PathBuf;
    fn path_from_object_hash(object_hash: &str) -> PathBuf;
    fn dir_path_from_object_hash(object_hash: &str) -> PathBuf;
}

impl<T: HasObjectHash> GitPath for T {
    fn path(&self) -> PathBuf {
        Self::path_from_object_hash(self.hash())
    }

    fn dir_path(&self) -> PathBuf {
        Self::dir_path_from_object_hash(self.hash())
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

enum GitObject {
    Blob(Blob),
    Tree(Tree),
}

struct Blob {
    object_hash: String,
    length: usize,
    contents: String,
}

#[derive(Debug)]
struct Tree {
    object_hash: String,
    entries: Vec<TreeEntry>,
}

impl Tree {
    fn new(object_hash: String, entries: Vec<TreeEntry>) -> Self {
        Self {
            object_hash,
            entries,
        }
    }

    fn from_tree_file(contents: &str) -> Self {
        let (header, rest) = contents.split_once('\0').unwrap();
        let length: usize = header.split(' ').nth(1).unwrap().parse().unwrap();
        let entries = rest
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                let (mode, rest) = line.split_once(' ').unwrap();
                let (name, object_hash) = rest.split_once('\0').unwrap();
                TreeEntry {
                    mode: mode.to_string(),
                    object_hash: object_hash.to_string(),
                    name: name.to_string(),
                }
            })
            .collect::<Vec<TreeEntry>>();

        let mut hasher = Sha1::new();
        hasher.update(contents.as_bytes());
        let result = hasher.finalize();
        let object_hash = format!("{:x}", result);

        Self {
            object_hash,
            entries,
        }
    }
}

impl HasObjectHash for Tree {
    fn hash(&self) -> &str {
        &self.object_hash
    }
}

#[derive(Debug)]
struct TreeEntry {
    mode: String,
    object_hash: String,
    name: String,
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

    // TODO: Writing git objects can move to trait and generic function can handle writing the output.
    fn header(&self) -> String {
        format!("blob {}\0", self.length)
    }

    fn as_bytes(&self) -> Vec<u8> {
        format!("{}{}", self.header(), self.contents).into_bytes()
    }
}

impl HasObjectHash for Blob {
    fn hash(&self) -> &str {
        &self.object_hash
    }
}

fn git_init() {
    fs::create_dir(".git").unwrap();
    fs::create_dir(".git/objects").unwrap();
    fs::create_dir(".git/refs").unwrap();
    fs::write(".git/HEAD", "ref: refs/heads/master\n").unwrap();
    println!("Initialized git directory")
}

fn load_git_object_from_hash(object: &str) -> GitObject {
    let path = Blob::path_from_object_hash(object);
    let file = fs::read(path).unwrap();

    let mut decompressed = ZlibDecoder::new(&file[..]);
    let mut buf = Vec::new();
    decompressed.read_to_end(&mut buf).unwrap();
    let contents = String::from_utf8_lossy(&buf);

    let split_contents = contents.split('\0').collect::<Vec<&str>>();
    let (tipe, length) = split_contents[0].split_once(' ').unwrap();
    let length: usize = length.parse().unwrap();

    // TODO: We should homogonise a deserialise/serialize function for each object struct, maybe as trait
    match tipe {
        "blob" => {
            let blob = Blob::new(
                object.to_string(),
                length,
                split_contents[1][0..length].to_string(),
            );
            GitObject::Blob(blob)
        }
        "tree" => GitObject::Tree(Tree::from_tree_file(&contents)),
        _ => panic!("Unknown object type: {}", tipe),
    }
}

fn git_cat_file(args: &CatFileArgs) {
    let object = load_git_object_from_hash(&args.object);

    match object {
        GitObject::Blob(blob) => {
            print!("{}", blob.contents);
        }
        GitObject::Tree(_) => todo!("git cat-file <tree> needs implementing!"),
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

fn git_ls_tree(args: &LsTreeArgs) {
    let object = load_git_object_from_hash(&args.object);

    match object {
        GitObject::Blob(_) => panic!("git ls-tree <blob> not implemented!"),
        GitObject::Tree(tree) => {
            for entry in tree.entries {
                println!("{}", entry.name);
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();

    // TODO: Macro this
    match &cli.command {
        Commands::Init => git_init(),
        Commands::CatFile(args) => {
            git_cat_file(args);
        }
        Commands::HashObject(args) => {
            git_hash_object(args);
        }
        Commands::LsTree(args) => {
            git_ls_tree(args);
        }
    }
}
