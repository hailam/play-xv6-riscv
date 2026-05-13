use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=kernel.ld");
    println!("cargo:rerun-if-changed=user/initcode.S");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let obj = out_dir.join("initcode.o");
    let elf = out_dir.join("initcode.elf");
    let bin = out_dir.join("initcode.bin");

    run("riscv64-elf-gcc", &[
        "-march=rv64gc", "-mabi=lp64",
        "-nostdlib", "-fno-pie", "-static",
        "-c", "-o", obj.to_str().unwrap(),
        "user/initcode.S",
    ]);

    run("riscv64-elf-ld", &[
        "-Ttext=0", "-N",
        "-o", elf.to_str().unwrap(),
        obj.to_str().unwrap(),
    ]);

    run("riscv64-elf-objcopy", &[
        "-O", "binary",
        elf.to_str().unwrap(), bin.to_str().unwrap(),
    ]);

    println!("cargo:rustc-env=INITCODE_BIN_PATH={}", bin.to_str().unwrap());
}

fn run(prog: &str, args: &[&str]) {
    let status = Command::new(prog)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {prog}: {e}"));
    assert!(status.success(), "{prog} {args:?} exited with {status}");
}
