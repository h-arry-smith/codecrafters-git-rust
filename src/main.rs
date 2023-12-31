use clap::Args;
use flate2::{read::ZlibDecoder, write::ZlibEncoder};
use sha1::{Digest, Sha1};
use std::fmt::Display;
use std::fmt::Write as FmtWrite;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::Write;
use std::str::FromStr;
use std::time::Instant;
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
    WriteTree,
    CommitTree(CommitTreeArgs),
}

#[derive(Debug, Args)]
struct CatFileArgs {
    #[arg(short)]
    pretty_print: bool,
    object: GitHash,
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
    object: GitHash,
}

#[derive(Debug, Args)]
struct CommitTreeArgs {
    tree: GitHash,
    #[arg(short)]
    parent: GitHash,
    #[arg(short)]
    message: String,
}

enum GitObject {
    Blob(Blob),
    Tree(Tree),
}

#[derive(Debug, Clone, Copy)]
struct GitHash {
    hash: [u8; 20],
}

impl FromStr for GitHash {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 40 {
            return Err("Hash must be 40 characters long");
        }

        let hash: [u8; 20] = hex::decode(s)
            .unwrap()
            .try_into()
            .expect("failed to decode");

        Ok(Self::new(hash))
    }
}

impl GitHash {
    fn new(hash: [u8; 20]) -> Self {
        Self { hash }
    }

    fn path(&self) -> PathBuf {
        let mut directory = String::with_capacity(2);
        for b in &self.hash[0..1] {
            write!(directory, "{:02x}", b).unwrap();
        }
        let mut filename = String::with_capacity(38);
        for b in &self.hash[1..20] {
            write!(filename, "{:02x}", b).unwrap();
        }
        format!(".git/objects/{}/{}", directory, filename).into()
    }

    fn dir_path(&self) -> PathBuf {
        let mut directory = String::with_capacity(2);
        for b in &self.hash[0..1] {
            write!(directory, "{:02x}", b).unwrap();
        }
        format!(".git/objects/{}", directory).into()
    }
}

impl Display for GitHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut hash = String::with_capacity(40);
        for b in &self.hash {
            write!(hash, "{:02x}", b).unwrap();
        }
        write!(f, "{}", hash)
    }
}

struct Blob {
    hash: GitHash,
    length: usize,
    contents: String,
}

#[derive(Debug)]
struct Tree {
    hash: GitHash,
    entries: Vec<TreeEntry>,
}

impl Tree {
    fn new(hash: GitHash, entries: Vec<TreeEntry>) -> Self {
        Self { hash, entries }
    }

    fn from_tree_file(contents: &str) -> Self {
        let hash = {
            let mut hasher = Sha1::new();
            hasher.update(contents.as_bytes());
            let result = hasher.finalize();
            GitHash::new(result.into())
        };

        let mut reader = BufReader::new(contents.as_bytes());
        let mut header = Vec::new();
        reader.read_until(b'\0', &mut header).unwrap();

        let mut entries = Vec::new();
        loop {
            let mut file_description = Vec::new();
            reader.read_until(b'\0', &mut file_description).unwrap();

            let mut hash: [u8; 20] = [0; 20];
            let read_result = reader.read_exact(&mut hash);

            if read_result.is_err() {
                break;
            }

            let file_description = String::from_utf8_lossy(&file_description);
            let (mode, name) = file_description.trim().split_once(' ').unwrap();

            let entry = TreeEntry {
                mode: mode.trim_end_matches(char::from(0)).to_string(),
                hash: GitHash::new(hash),
                name: name.trim_end_matches(char::from(0)).to_string(),
            };

            entries.push(entry);

            if reader.buffer().len() < 20 {
                break;
            }
        }

        Self::new(hash, entries)
    }

    fn from_entries(entries: Vec<TreeEntry>) -> Self {
        let contents = Self::contents_from_entries(&entries);

        let mut hasher = Sha1::new();
        hasher.update(contents.as_bytes());
        let result = hasher.finalize();
        let hash = GitHash::new(result.into());

        Self { hash, entries }
    }

    fn contents(&self) -> String {
        Self::contents_from_entries(&self.entries)
    }

    fn body_from_entries(entries: &[TreeEntry]) -> String {
        let mut contents = String::new();

        for entry in entries.iter() {
            contents.push_str(&format!("{} {}\0", entry.mode, entry.name));

            let hash_string = unsafe { std::str::from_utf8_unchecked(&entry.hash.hash) };

            contents.push_str(hash_string);
        }

        contents
    }

    fn contents_from_entries(entries: &[TreeEntry]) -> String {
        let body = Self::body_from_entries(entries);
        format!("tree {}\0{}", body.len(), body)
    }

    fn tree_from_directory(path: PathBuf) -> Self {
        let mut files_and_directories_in_path = fs::read_dir(path).unwrap().collect::<Vec<_>>();

        files_and_directories_in_path.sort_by(|a, b| {
            let a = a.as_ref().unwrap();
            let b = b.as_ref().unwrap();

            a.file_name()
                .to_str()
                .unwrap()
                .cmp(b.file_name().to_str().unwrap())
        });

        let files_and_directories_in_path = files_and_directories_in_path.iter().filter(|entry| {
            if let Ok(entry) = entry {
                entry.file_name().to_str().unwrap() != ".git"
            } else {
                false
            }
        });

        let mut entries = Vec::new();

        for entry in files_and_directories_in_path {
            let entry = entry.as_ref().unwrap();
            let path = entry.path();
            let metadata = entry.metadata().unwrap();

            if metadata.is_dir() {
                let tree = Self::tree_from_directory(path);
                let tree_entry = TreeEntry {
                    mode: "40000".to_string(),
                    hash: tree.hash,
                    name: entry.file_name().to_str().unwrap().to_string(),
                };

                entries.push(tree_entry);
            } else {
                let contents = fs::read_to_string(path).unwrap();
                let blob = Blob::from_contents(&contents);

                let tree_entry = TreeEntry {
                    mode: "100644".to_string(),
                    hash: blob.hash,
                    name: entry.file_name().to_str().unwrap().to_string(),
                };

                entries.push(tree_entry);
            }
        }

        Self::from_entries(entries)
    }
}

#[derive(Debug)]
struct TreeEntry {
    mode: String,
    hash: GitHash,
    name: String,
}

impl Blob {
    fn new(hash: GitHash, length: usize, contents: String) -> Self {
        Self {
            hash,
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
        let hash = GitHash::new(result.into());

        Self {
            hash,
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

fn git_init() {
    fs::create_dir(".git").unwrap();
    fs::create_dir(".git/objects").unwrap();
    fs::create_dir(".git/refs").unwrap();
    fs::write(".git/HEAD", "ref: refs/heads/master\n").unwrap();
    println!("Initialized git directory")
}

fn load_git_object_from_hash(hash: GitHash) -> GitObject {
    let file = fs::read(hash.path()).unwrap();

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
            let blob = Blob::new(hash, length, split_contents[1][0..length].to_string());
            GitObject::Blob(blob)
        }
        "tree" => GitObject::Tree(Tree::from_tree_file(&contents)),
        _ => panic!("Unknown object type: {}", tipe),
    }
}

fn git_cat_file(args: &CatFileArgs) {
    let object = load_git_object_from_hash(args.object);

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
        fs::create_dir_all(blob.hash.dir_path()).unwrap();

        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&blob.as_bytes()).unwrap();

        let compressed = encoder.finish().unwrap();
        fs::write(blob.hash.path(), compressed).unwrap();
    }

    print!("{}", blob.hash);
}

fn git_ls_tree(args: &LsTreeArgs) {
    let object = load_git_object_from_hash(args.object);

    match object {
        GitObject::Blob(_) => panic!("git ls-tree <blob> not implemented!"),
        GitObject::Tree(tree) => {
            for entry in tree.entries {
                println!("{}", entry.name);
            }
        }
    }
}

fn git_write_tree() {
    let tree = Tree::tree_from_directory(PathBuf::from("."));
    fs::create_dir_all(tree.hash.dir_path()).unwrap();

    let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(tree.contents().as_bytes()).unwrap();

    let compressed = encoder.finish().unwrap();
    fs::write(tree.hash.path(), compressed).unwrap();

    println!("{}", tree.hash);
}

fn git_commit_tree(args: &CommitTreeArgs) {
    let unix_timestamp = Instant::now().elapsed().as_secs();
    let time_zone = "+0000";
    let author = format!(
        "{} <{}> {} {}",
        "Harry Smith", "test@test.org", unix_timestamp, time_zone
    );
    let contents = format!(
        "tree {}\nparent {}\nauthor {}\ncommitter {}\n\n{}\n",
        args.tree, args.parent, author, author, args.message
    );

    // TODO: All this write logic is duplicated everywhere, and could be tidied up.
    let hash = {
        let mut hasher = Sha1::new();
        hasher.update(contents.as_bytes());
        let result = hasher.finalize();
        GitHash::new(result.into())
    };

    let header = format!("commit {}\0", contents.len());

    fs::create_dir_all(hash.dir_path()).unwrap();
    let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(header.as_bytes()).unwrap();
    encoder.write_all(contents.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    fs::write(hash.path(), compressed).unwrap();

    println!("{}", hash);
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
        Commands::WriteTree => git_write_tree(),
        Commands::CommitTree(args) => {
            git_commit_tree(args);
        }
    }
}
