use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy)]
enum Lang {
    Asm,
    C,
}

/// Per-program C runtime choice. `Ulib` is our hand-rolled tiny
/// runtime (the original xv6-style one). `Picolibc` links against
/// the meson-built picolibc archive — adds `<stdio.h>`/`printf`/
/// `malloc`/etc. but pulls ~10 KB into a stripped binary.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Runtime {
    Ulib,
    Picolibc,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arch {
    Riscv64,
    Aarch64,
}

fn current_arch() -> Arch {
    match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("riscv64") => Arch::Riscv64,
        Ok("aarch64") => Arch::Aarch64,
        Ok(other) => panic!("unsupported target arch {other}"),
        Err(e) => panic!("CARGO_CFG_TARGET_ARCH unset: {e}"),
    }
}

fn rust_lld_path() -> PathBuf {
    let sysroot = String::from_utf8(
        Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .expect("rustc --print sysroot")
            .stdout,
    )
    .unwrap();
    let host = String::from_utf8(
        Command::new("rustc")
            .args(["-vV"])
            .output()
            .expect("rustc -vV")
            .stdout,
    )
    .unwrap();
    let host = host
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .unwrap();
    PathBuf::from(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join(host)
        .join("bin")
        .join("gcc-ld")
        .join("ld.lld")
}

fn rust_objcopy_path() -> PathBuf {
    let sysroot = String::from_utf8(
        Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .expect("rustc --print sysroot")
            .stdout,
    )
    .unwrap();
    let host = String::from_utf8(
        Command::new("rustc")
            .args(["-vV"])
            .output()
            .expect("rustc -vV")
            .stdout,
    )
    .unwrap();
    let host = host
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .unwrap();
    PathBuf::from(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join(host)
        .join("bin")
        .join("llvm-objcopy")
}

fn main() {
    println!("cargo:rerun-if-changed=kernel.ld");
    println!("cargo:rerun-if-changed=kernel-aarch64.ld");
    println!("cargo:rerun-if-changed=user/user.ld");
    println!("cargo:rerun-if-changed=user/user-aarch64.ld");

    let arch = current_arch();
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    // target/user/<arch>/ for the per-arch ELFs. mkfs reads from
    // here. Riscv keeps the existing target/user/ for backwards
    // compat.
    let target_dir = out_dir.ancestors().nth(5).map(|p| p.to_path_buf());
    let user_dir = target_dir.as_ref().map(|t| match arch {
        Arch::Riscv64 => t.join("user"),
        Arch::Aarch64 => t.join("user-aarch64"),
    });
    if let Some(ud) = user_dir.as_ref() {
        let _ = std::fs::create_dir_all(ud);
    }

    // ---- ulib + umalloc + printf — shared user runtime ----
    // ulib-asm is the arch-specific syscall stubs (ulib.S on riscv,
    // ulib-aarch64.S on aarch64). ulib.c/umalloc.c/printf.c are
    // arch-independent — just C compiled for the target.
    let ulib_asm_obj = compile_to_obj(arch, &out_dir, "ulib", Lang::Asm);
    let ulib_c_obj = compile_to_obj(arch, &out_dir, "ulib", Lang::C);
    let umalloc_obj = compile_to_obj(arch, &out_dir, "umalloc", Lang::C);
    let printf_obj = compile_to_obj(arch, &out_dir, "printf", Lang::C);
    let user_objs: Vec<PathBuf> =
        vec![ulib_asm_obj.clone(), ulib_c_obj.clone(), umalloc_obj, printf_obj];
    // For picolibc-using programs we want ONLY the syscall stubs +
    // `_start` from ulib — picolibc supplies its own printf,
    // malloc, strlen, memset, etc. Linking ulib's printf.o /
    // umalloc.o ahead of libc.a would silently shadow picolibc's
    // versions (object-files-before-archives), giving you the
    // minimal `%d %s` formatter instead of picolibc's full one.
    let pico_runtime_objs: Vec<PathBuf> = vec![ulib_asm_obj, ulib_c_obj];

    // initcode is the only user binary we currently emit for
    // aarch64. The rest of the C suite (sh, ls, cat, …) builds
    // riscv-only.
    let initcode_name = match arch {
        Arch::Riscv64 => "initcode",
        Arch::Aarch64 => "initcode-aarch64",
    };
    let initcode_bin =
        build_user_program(arch, &out_dir, initcode_name, Lang::Asm, false, &user_objs);
    println!(
        "cargo:rustc-env=INITCODE_BIN_PATH={}",
        initcode_bin.display()
    );
    if let Some(ud) = user_dir.as_ref() {
        let _ = std::fs::copy(&initcode_bin, ud.join("initcode.elf"));
    }

    // hello.S and pipetest.S are riscv-only assembly demos — skip on
    // aarch64. The rest is C, builds for both arches via the ulib
    // runtime. picohello is the first user binary built against
    // picolibc — uses `<stdio.h>`/`printf`, linked through xmake's
    // libc.a.
    let programs: &[(&str, &str, Lang, bool, &[Arch], Runtime)] = &[
        ("echo", "ECHO_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("hello", "HELLO_BIN_PATH", Lang::Asm, false, &[Arch::Riscv64], Runtime::Ulib),
        ("pipetest", "PIPETEST_BIN_PATH", Lang::Asm, false, &[Arch::Riscv64], Runtime::Ulib),
        ("sh", "SH_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("cat", "CAT_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("ls", "LS_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("mkdir", "MKDIR_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("rm", "RM_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("wr", "WR_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("kill", "KILL_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("killtest", "KILLTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("malloctest", "MALLOCTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("smptest", "SMPTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("ln", "LN_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("faulttest", "FAULTTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("xv6test", "XV6TEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("lazytest", "LAZYTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("usertests", "USERTESTS_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("seektest", "SEEKTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("chmodtest", "CHMODTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("credtest", "CREDTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("cloexectest", "CLOEXECTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("trunctest", "TRUNCTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("stattime", "STATTIME_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("sigtest", "SIGTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("sigactest", "SIGACTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("sigmasktest", "SIGMASKTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("fdfiletest", "FDFILETEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("alarmtest", "ALARMTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("ctimetest", "CTIMETEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("envtest", "ENVTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("posix6test", "POSIX6TEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("mmaptest", "MMAPTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("symlinktest", "SYMLINKTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("ioctltest", "IOCTLTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("polltest", "POLLTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("pwd", "PWD_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        ("env", "ENV_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64], Runtime::Ulib),
        // picolibc-using programs. The compile/link path runs
        // `xmake build picolibc-<arch>` lazily if libc.a isn't
        // there yet. Programs marked Runtime::Picolibc get
        // <stdio.h>, printf, malloc, FILE*, etc.
        ("picohello", "PICOHELLO_BIN_PATH", Lang::C, false, &[Arch::Riscv64, Arch::Aarch64], Runtime::Picolibc),
        ("picotest", "PICOTEST_BIN_PATH", Lang::C, false, &[Arch::Riscv64, Arch::Aarch64], Runtime::Picolibc),
    ];
    for (name, env_var, lang, with_ulib, supported_archs, runtime) in programs {
        if supported_archs.contains(&arch) {
            let bin = match runtime {
                Runtime::Ulib => build_user_program(
                    arch, &out_dir, name, *lang, *with_ulib, &user_objs,
                ),
                Runtime::Picolibc => build_user_program_picolibc(
                    arch, &out_dir, name, &pico_runtime_objs,
                ),
            };
            println!("cargo:rustc-env={env_var}={}", bin.display());
            if let Some(ud) = user_dir.as_ref() {
                let _ = std::fs::copy(&bin, ud.join(format!("{name}.elf")));
            }
        } else {
            // Satisfy `include_bytes!()` so the kernel still
            // compiles. The bytes will never be referenced because
            // the corresponding embed entry is gated by arch.
            println!("cargo:rustc-env={env_var}={}", initcode_bin.display());
        }
    }
}

fn compile_to_obj(arch: Arch, out_dir: &Path, name: &str, lang: Lang) -> PathBuf {
    // ulib.S is arch-specific. For aarch64 we redirect to ulib-aarch64.S
    // but still emit ulib-S.o so the linker sees a single name. Other
    // assembly stubs (hello.S, pipetest.S, initcode.S) are riscv-only;
    // their aarch64 equivalents have a distinct name.
    let effective_name = match (arch, lang, name) {
        (Arch::Aarch64, Lang::Asm, "ulib") => "ulib-aarch64",
        _ => name,
    };
    let (src, obj_name) = match lang {
        Lang::Asm => (
            format!("user/{effective_name}.S"),
            format!("{name}-S.o"),
        ),
        Lang::C => (
            format!("user/{name}.c"),
            format!("{name}-c.o"),
        ),
    };
    println!("cargo:rerun-if-changed={src}");
    let obj = out_dir.join(obj_name);

    match (arch, lang) {
        (Arch::Riscv64, Lang::Asm) => run("riscv64-elf-gcc", &[
            "-march=rv64gc", "-mabi=lp64",
            "-nostdlib", "-fno-pie", "-static",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
        (Arch::Riscv64, Lang::C) => run("riscv64-elf-gcc", &[
            "-march=rv64gc", "-mabi=lp64", "-mcmodel=medany",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding", "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-fno-builtin",
            "-Os", "-Wall",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
        (Arch::Aarch64, Lang::Asm) => run("clang", &[
            "--target=aarch64-none-elf", "-march=armv8-a",
            "-mgeneral-regs-only",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
        (Arch::Aarch64, Lang::C) => run("clang", &[
            "--target=aarch64-none-elf", "-march=armv8-a",
            "-mgeneral-regs-only",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding", "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-fno-builtin",
            "-Os", "-Wall",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
    }
    obj
}

fn build_user_program(
    arch: Arch,
    out_dir: &Path,
    name: &str,
    lang: Lang,
    with_ulib: bool,
    runtime_objs: &[PathBuf],
) -> PathBuf {
    let obj = compile_to_obj(arch, out_dir, name, lang);
    let elf = out_dir.join(format!("{name}.elf"));
    let stripped = out_dir.join(format!("{name}-stripped.elf"));

    let linker_script = match arch {
        Arch::Riscv64 => "user/user.ld",
        Arch::Aarch64 => "user/user-aarch64.ld",
    };

    let mut ld_args = vec![
        "-T".to_string(),
        linker_script.to_string(),
        "-N".to_string(),
        "-o".to_string(),
        elf.to_str().unwrap().to_string(),
    ];
    if with_ulib {
        for o in runtime_objs {
            ld_args.push(o.to_str().unwrap().to_string());
        }
    }
    ld_args.push(obj.to_str().unwrap().to_string());
    let ld_args_ref: Vec<&str> = ld_args.iter().map(|s| s.as_str()).collect();

    match arch {
        Arch::Riscv64 => run("riscv64-elf-ld", &ld_args_ref),
        Arch::Aarch64 => {
            let lld = rust_lld_path();
            run(lld.to_str().unwrap(), &ld_args_ref)
        }
    }

    // Strip everything that isn't needed at runtime.
    match arch {
        Arch::Riscv64 => run("riscv64-elf-objcopy", &[
            "--strip-all",
            elf.to_str().unwrap(),
            stripped.to_str().unwrap(),
        ]),
        Arch::Aarch64 => {
            let oc = rust_objcopy_path();
            run(oc.to_str().unwrap(), &[
                "--strip-all",
                elf.to_str().unwrap(),
                stripped.to_str().unwrap(),
            ]);
        }
    }

    stripped
}

fn run(prog: &str, args: &[&str]) {
    let status = Command::new(prog)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {prog}: {e}"));
    assert!(status.success(), "{prog} {args:?} exited with {status}");
}

/// Repo root (parent of `target/` and `crates/`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("canonicalize repo root")
}

/// `build/picolibc-<arch>/install` produced by `xmake build
/// picolibc-<arch>`. Headers under `include/`, archives under
/// `lib/` (libc.a, libm.a, …).
fn picolibc_install_dir(arch: Arch) -> PathBuf {
    let arch_dir = match arch {
        Arch::Riscv64 => "picolibc-riscv64",
        Arch::Aarch64 => "picolibc-aarch64",
    };
    repo_root().join("build").join(arch_dir).join("install")
}

/// Ensure `build/picolibc-<arch>/install/lib/libc.a` exists, running
/// `xmake build picolibc-<arch>` if not. xmake is responsible for
/// fetching picolibc upstream + meson setup + ninja install.
fn ensure_picolibc_built(arch: Arch) {
    let libc = picolibc_install_dir(arch).join("lib").join("libc.a");
    if libc.exists() {
        return;
    }
    let target = match arch {
        Arch::Riscv64 => "picolibc-riscv64",
        Arch::Aarch64 => "picolibc-aarch64",
    };
    println!("cargo:warning=building picolibc for {target} (one-time)");
    let status = Command::new("xmake")
        .current_dir(repo_root())
        .args(["build", "-y", target])
        .status()
        .unwrap_or_else(|e| panic!(
            "failed to run xmake (install via `brew install xmake`): {e}"
        ));
    assert!(status.success(), "xmake build {target} failed");
    assert!(libc.exists(), "xmake completed but {} is missing", libc.display());
}

fn build_user_program_picolibc(
    arch: Arch,
    out_dir: &Path,
    name: &str,
    runtime_objs: &[PathBuf],
) -> PathBuf {
    ensure_picolibc_built(arch);
    let install = picolibc_install_dir(arch);
    let include = install.join("include");
    let lib_dir = install.join("lib");
    let libc_a = lib_dir.join("libc.a");

    let src = format!("user/{name}.c");
    println!("cargo:rerun-if-changed={src}");
    println!("cargo:rerun-if-changed={}", libc_a.display());

    let obj = out_dir.join(format!("{name}-pico.o"));
    let inc_arg = format!("-I{}", include.display());
    match arch {
        Arch::Riscv64 => run("riscv64-elf-gcc", &[
            "-march=rv64gc", "-mabi=lp64", "-mcmodel=medany",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding", "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-Os", "-Wall",
            &inc_arg,
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
        Arch::Aarch64 => run("clang", &[
            "--target=aarch64-none-elf", "-march=armv8-a",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding", "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-Os", "-Wall",
            &inc_arg,
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
    }

    let elf = out_dir.join(format!("{name}.elf"));
    let stripped = out_dir.join(format!("{name}-stripped.elf"));
    let linker_script = match arch {
        Arch::Riscv64 => "user/user.ld",
        Arch::Aarch64 => "user/user-aarch64.ld",
    };

    // ulib runtime objects supply `_start`, the syscall stubs that
    // picolibc's posix-console glue calls (read/write/lseek/close/
    // _exit/sbrk/...), and `main` invocation. picolibc itself is the
    // last archive — we pull `printf`, `malloc`, etc. out of it.
    let mut ld_args: Vec<String> = vec![
        "-T".into(), linker_script.into(),
        "-N".into(),
        "-o".into(), elf.to_str().unwrap().into(),
    ];
    for o in runtime_objs {
        ld_args.push(o.to_str().unwrap().into());
    }
    ld_args.push(obj.to_str().unwrap().into());
    ld_args.push(libc_a.to_str().unwrap().into());
    // picolibc's dtoa-ryu uses 128-bit shifts (__lshrti3, __ashlti3)
    // on riscv64 — supplied by libgcc.a. aarch64 has the same
    // helpers in compiler-rt, which clang's driver finds; but here
    // we link with `ld.lld` directly, so we have to add the
    // compiler-rt builtins archive explicitly. Resolved lazily so
    // riscv builds don't pay for a clang query.
    match arch {
        Arch::Riscv64 => {
            let libgcc_out = Command::new("riscv64-elf-gcc")
                .args(["-march=rv64gc", "-mabi=lp64", "-print-libgcc-file-name"])
                .output()
                .expect("riscv64-elf-gcc -print-libgcc-file-name");
            let libgcc = String::from_utf8(libgcc_out.stdout)
                .unwrap().trim().to_string();
            ld_args.push(libgcc);
        }
        Arch::Aarch64 => {
            // clang's builtins archive — aarch64 build of picolibc
            // emits long-double soft-float helpers (__floatunditf,
            // __divtf3, ...) that live here. The path is reported
            // by `clang --target=aarch64-none-elf
            // -print-libgcc-file-name`.
            let cr_out = Command::new("clang")
                .args(["--target=aarch64-none-elf", "-print-libgcc-file-name"])
                .output()
                .expect("clang -print-libgcc-file-name");
            let cr = String::from_utf8(cr_out.stdout).unwrap().trim().to_string();
            if !cr.is_empty() && std::path::Path::new(&cr).exists() {
                ld_args.push(cr);
            }
        }
    }
    let ld_args_ref: Vec<&str> = ld_args.iter().map(|s| s.as_str()).collect();
    match arch {
        Arch::Riscv64 => run("riscv64-elf-ld", &ld_args_ref),
        Arch::Aarch64 => {
            let lld = rust_lld_path();
            run(lld.to_str().unwrap(), &ld_args_ref)
        }
    }

    match arch {
        Arch::Riscv64 => run("riscv64-elf-objcopy", &[
            "--strip-all",
            elf.to_str().unwrap(),
            stripped.to_str().unwrap(),
        ]),
        Arch::Aarch64 => {
            let oc = rust_objcopy_path();
            run(oc.to_str().unwrap(), &[
                "--strip-all",
                elf.to_str().unwrap(),
                stripped.to_str().unwrap(),
            ]);
        }
    }

    stripped
}
