use clap::Args;
use flate2::read::ZlibDecoder;
use std::fs;
use std::io::prelude::*;

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
}

#[derive(Debug, Args)]
struct CatFileArgs {
    #[arg(short)]
    pretty_print: bool,
    object: String,
}

enum GitObject {
    Blob { length: usize, contents: String },
}

fn git_init() {
    fs::create_dir(".git").unwrap();
    fs::create_dir(".git/objects").unwrap();
    fs::create_dir(".git/refs").unwrap();
    fs::write(".git/HEAD", "ref: refs/heads/master\n").unwrap();
    println!("Initialized git directory")
}

fn git_cat_file(args: &CatFileArgs) {
    let directory = args.object.chars().take(2).collect::<String>();
    let filename = args.object.chars().skip(2).collect::<String>();
    let path = format!(".git/objects/{}/{}", directory, filename);

    let file = fs::read(path).unwrap();

    let mut decompressed = ZlibDecoder::new(&*file);
    let mut contents = String::new();
    decompressed.read_to_string(&mut contents).unwrap();

    let contents = contents.split('\0').collect::<Vec<&str>>();
    let (tipe, length) = contents[0].split_once(' ').unwrap();
    let length: usize = length.parse().unwrap();

    let object = match tipe {
        "blob" => GitObject::Blob {
            length,
            contents: contents[1][0..length].to_string(),
        },
        _ => panic!("Unknown object type: {}", tipe),
    };

    match object {
        GitObject::Blob {
            length: _,
            contents,
        } => {
            print!("{}", contents);
        }
    }
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Init => git_init(),
        Commands::CatFile(args) => {
            git_cat_file(args);
        }
    }
}
