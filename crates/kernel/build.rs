use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=kernel.ld");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for (name, env_var) in [
        ("initcode", "INITCODE_BIN_PATH"),
        ("echo", "ECHO_BIN_PATH"),
        ("hello", "HELLO_BIN_PATH"),
        ("pipetest", "PIPETEST_BIN_PATH"),
    ] {
        let bin = build_user_program(&out_dir, name);
        println!("cargo:rustc-env={env_var}={}", bin.display());
    }
}

fn build_user_program(out_dir: &Path, name: &str) -> PathBuf {
    let src = format!("user/{name}.S");
    let obj = out_dir.join(format!("{name}.o"));
    let elf = out_dir.join(format!("{name}.elf"));
    let bin = out_dir.join(format!("{name}.bin"));

    println!("cargo:rerun-if-changed={src}");

    run("riscv64-elf-gcc", &[
        "-march=rv64gc", "-mabi=lp64",
        "-nostdlib", "-fno-pie", "-static",
        "-c", "-o", obj.to_str().unwrap(),
        &src,
    ]);
    run("riscv64-elf-ld", &[
        "-Ttext=0", "-N",
        "-o", elf.to_str().unwrap(),
        obj.to_str().unwrap(),
    ]);
    run("riscv64-elf-objcopy", &[
        "-O", "binary",
        elf.to_str().unwrap(),
        bin.to_str().unwrap(),
    ]);
    bin
}

fn run(prog: &str, args: &[&str]) {
    let status = Command::new(prog)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {prog}: {e}"));
    assert!(status.success(), "{prog} {args:?} exited with {status}");
}
