-- xv6-rs build orchestration (phase 1: picolibc only).
--
-- Scope today: fetch picolibc source + build a libc.a + crt0.o for
-- each of our two arches, ready for linking into user binaries.
-- The kernel itself still builds via cargo + Makefile.
--
-- Future phases will replace the Makefile entirely; everything
-- here is laid out so that adding kernel + user-binary + fs.img
-- targets is incremental, not a rewrite.

set_project("xv6-rs")
set_xmakever("3.0.0")

-- Host build tools — xmake fetches these into ~/.xmake on first
-- build so we don't require global meson/ninja installs.
add_requires("meson", "ninja")

-- Picolibc upstream tag we build against. Bumping this rebuilds.
local PICOLIBC_TAG = "1.8.11"

-- Common meson options we use for both arches. Mirror in both
-- target callbacks.
local PICOLIBC_OPTS = {
    "-Dsemihost=false",
    "-Dmultilib=false",
    "-Dposix-console=true",
    "-Dtests=false",
    "-Dnative-tests=false",
    "-Dio-long-long=true",
    -- picocrt=false: we keep our existing ulib.S `_start` and use
    -- picolibc as a pure libc.a. Enabling picocrt drags in
    -- semihost.h references from the unselected crt0-semihost
    -- variants picolibc builds unconditionally.
    "-Dpicocrt=false",
    "-Denable-malloc=true",
    -- TLS off: we don't run threads, and aarch64 picolibc tags
    -- `errno` as STT_TLS by default — which requires a PT_TLS
    -- segment in the linker script + `_start` TLS init. Easier
    -- to just turn it off; errno falls back to a plain global.
    "-Dthread-local-storage=false",
}

-- Build helper. Because xmake's sandbox doesn't share the `os`
-- table with module-level helpers, the actual file/process work
-- has to live inside the on_build closure. This function only
-- composes argument vectors — no I/O.
local function picolibc_setup_args(target, arch, cross_file_name, build_dir, install_dir, src)
    local cross_file = path.join(
        os.projectdir(), "third_party", cross_file_name
    )
    local args = {"setup", "--cross-file=" .. cross_file,
                  "--prefix=" .. install_dir}
    for _, o in ipairs(PICOLIBC_OPTS) do table.insert(args, o) end
    table.insert(args, build_dir)
    table.insert(args, src)
    return args
end

target("picolibc-riscv64")
    set_kind("phony")
    add_packages("meson", "ninja")
    on_build(function (target)
        local arch = "riscv64"
        local cross_file_name = "cross-riscv64-xv6rs.txt"
        local src = path.join(os.projectdir(), "third_party", "picolibc-src")
        if not os.isdir(src) then
            print(string.format("xmake: fetching picolibc %s ...", PICOLIBC_TAG))
            os.vrunv("git", {
                "clone", "--depth=1", "--branch", PICOLIBC_TAG,
                "https://github.com/picolibc/picolibc.git", src,
            })
        end
        -- Find the xmake-installed meson/ninja bin dirs. target:pkg
        -- returns nil for phony targets in xmake 3.x, so we use
        -- os.match (which DOES expand glob patterns) on
        -- ~/.xmake/packages/{m,n}/{meson,ninja}/<ver>/<hash>/bin/.
        local function find_pkg_bin(prefix, name)
            local pattern = path.join(
                os.getenv("HOME"), ".xmake/packages",
                prefix, name, "*", "*", "bin", name
            )
            local matches = os.match(pattern)
            return matches[#matches]
        end
        local meson_bin = find_pkg_bin("m", "meson")
        local ninja_bin = find_pkg_bin("n", "ninja")
        assert(meson_bin, "meson not found in ~/.xmake/packages")
        assert(ninja_bin, "ninja not found in ~/.xmake/packages")
        -- clang invokes its bundled linker by looking up `ld.lld`
        -- in PATH. Our cross file points at rustup's ld.lld for
        -- the aarch64 build — also make sure clang's own driver
        -- can find it.
        local rustup_bin = path.join(
            os.getenv("HOME"),
            ".rustup/toolchains/nightly-aarch64-apple-darwin",
            "lib/rustlib/aarch64-apple-darwin/bin/gcc-ld"
        )
        local envs = {
            PATH = table.concat({
                path.directory(meson_bin),
                path.directory(ninja_bin),
                rustup_bin,
                os.getenv("PATH") or "",
            }, path.envsep()),
        }
        local build_dir = path.join(os.projectdir(), "build", "picolibc-" .. arch)
        local install_dir = path.join(build_dir, "install")
        if not os.isfile(path.join(build_dir, "build.ninja")) then
            os.mkdir(build_dir)
            -- Pass the absolute meson_bin path — xmake's os.vrunv
            -- does PATH lookup in the PARENT env, not in the child's
            -- envs, so a bare "meson" doesn't resolve even though
            -- we set envs.PATH.
            os.vrunv(meson_bin,
                picolibc_setup_args(target, arch, cross_file_name, build_dir, install_dir, src),
                {envs = envs})
        end
        os.vrunv(ninja_bin, {"-C", build_dir, "install"}, {envs = envs})
    end)

target("picolibc-aarch64")
    set_kind("phony")
    add_packages("meson", "ninja")
    on_build(function (target)
        local arch = "aarch64"
        local cross_file_name = "cross-aarch64-xv6rs.txt"
        local src = path.join(os.projectdir(), "third_party", "picolibc-src")
        if not os.isdir(src) then
            print(string.format("xmake: fetching picolibc %s ...", PICOLIBC_TAG))
            os.vrunv("git", {
                "clone", "--depth=1", "--branch", PICOLIBC_TAG,
                "https://github.com/picolibc/picolibc.git", src,
            })
        end
        -- Find the xmake-installed meson/ninja bin dirs. target:pkg
        -- returns nil for phony targets in xmake 3.x, so we use
        -- os.match (which DOES expand glob patterns) on
        -- ~/.xmake/packages/{m,n}/{meson,ninja}/<ver>/<hash>/bin/.
        local function find_pkg_bin(prefix, name)
            local pattern = path.join(
                os.getenv("HOME"), ".xmake/packages",
                prefix, name, "*", "*", "bin", name
            )
            local matches = os.match(pattern)
            return matches[#matches]
        end
        local meson_bin = find_pkg_bin("m", "meson")
        local ninja_bin = find_pkg_bin("n", "ninja")
        assert(meson_bin, "meson not found in ~/.xmake/packages")
        assert(ninja_bin, "ninja not found in ~/.xmake/packages")
        -- clang invokes its bundled linker by looking up `ld.lld`
        -- in PATH. Our cross file points at rustup's ld.lld for
        -- the aarch64 build — also make sure clang's own driver
        -- can find it.
        local rustup_bin = path.join(
            os.getenv("HOME"),
            ".rustup/toolchains/nightly-aarch64-apple-darwin",
            "lib/rustlib/aarch64-apple-darwin/bin/gcc-ld"
        )
        local envs = {
            PATH = table.concat({
                path.directory(meson_bin),
                path.directory(ninja_bin),
                rustup_bin,
                os.getenv("PATH") or "",
            }, path.envsep()),
        }
        local build_dir = path.join(os.projectdir(), "build", "picolibc-" .. arch)
        local install_dir = path.join(build_dir, "install")
        if not os.isfile(path.join(build_dir, "build.ninja")) then
            os.mkdir(build_dir)
            -- Pass the absolute meson_bin path — xmake's os.vrunv
            -- does PATH lookup in the PARENT env, not in the child's
            -- envs, so a bare "meson" doesn't resolve even though
            -- we set envs.PATH.
            os.vrunv(meson_bin,
                picolibc_setup_args(target, arch, cross_file_name, build_dir, install_dir, src),
                {envs = envs})
        end
        os.vrunv(ninja_bin, {"-C", build_dir, "install"}, {envs = envs})
    end)
