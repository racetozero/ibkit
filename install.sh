#!/bin/sh
# Install ibkit (the `ibkit` command and the JVM agent it loads into IB
# Gateway) from a GitHub release on macOS or glibc Linux:
#
#   curl -fsSL https://raw.githubusercontent.com/racetozero/ibkit/main/install.sh | sh
#
# IBKIT_VERSION     release to install (default: the latest)
# IBKIT_INSTALL_DIR directory for the binary (default: ~/.local/bin); the
#                   agent goes to the lib directory beside it (~/.local/lib)
# IBKIT_TARGET      Rust target triple (default: this machine)
# IBKIT_NO_MODIFY_PATH=1  leave shell startup files unchanged
# IBKIT_REPOSITORY, IBKIT_RELEASES_URL  another release source (with
#                   IBKIT_RELEASES_URL, set IBKIT_VERSION too)
set -eu

REPOSITORY="${IBKIT_REPOSITORY:-racetozero/ibkit}"
RELEASES_URL="${IBKIT_RELEASES_URL:-https://github.com/$REPOSITORY/releases}"

say() {
    printf '%s\n' "$*" >&2
}

fail() {
    say "ibkit installer: $*"
    exit 1
}

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

github_cli_ready() {
    [ -z "${IBKIT_RELEASES_URL:-}" ] &&
        command_exists gh &&
        gh auth status --hostname github.com >/dev/null 2>&1
}

normalize_tag() {
    requested="$1"
    case "$requested" in
        v*) tag="$requested" ;;
        *) tag="v$requested" ;;
    esac
    version="${tag#v}"
    case "$version" in
        '' | *[!0-9A-Za-z.+-]*) fail "IBKIT_VERSION is $requested. Use a release version such as 1.2.3, or unset it for the latest release." ;;
    esac
}

resolve_tag() {
    requested="${IBKIT_VERSION:-}"
    if [ -n "$requested" ] && [ "$requested" != "latest" ]; then
        normalize_tag "$requested"
        return
    fi
    if github_cli_ready; then
        resolved="$(gh release view --repo "$REPOSITORY" --json tagName --jq .tagName)" ||
            fail "gh could not find the latest release of $REPOSITORY. Set IBKIT_VERSION, or check gh auth status."
        normalize_tag "$resolved"
        return
    fi
    [ -z "${IBKIT_RELEASES_URL:-}" ] ||
        fail "IBKIT_RELEASES_URL is set without IBKIT_VERSION. Set IBKIT_VERSION to the release to install."
    command_exists curl ||
        fail "curl is not installed, and it finds the latest release. Install curl, or set IBKIT_VERSION."
    final_url="$(curl --proto '=https' --tlsv1.2 -fsSL -o /dev/null -w '%{url_effective}' "$RELEASES_URL/latest")" ||
        fail "could not reach $RELEASES_URL/latest. Check the network connection, or set IBKIT_VERSION."
    resolved="${final_url%/}"
    normalize_tag "${resolved##*/}"
}

detect_target() {
    if [ -n "${IBKIT_TARGET:-}" ]; then
        target="$IBKIT_TARGET"
    else
        os="$(uname -s)"
        machine="$(uname -m)"
        case "$machine" in
            arm64 | aarch64) architecture="aarch64" ;;
            x86_64 | amd64) architecture="x86_64" ;;
            *) fail "this processor is $machine. ibkit releases are for x86_64 and arm64." ;;
        esac
        case "$os" in
            Darwin) target="$architecture-apple-darwin" ;;
            Linux)
                # The agent loads into Gateway's JVM, which IBKR ships for glibc.
                if command_exists ldd && ldd --version 2>&1 | grep -qi musl; then
                    fail "this Linux uses musl libc. IB Gateway and the ibkit agent need glibc; use a glibc distribution or the ibkit-gateway container image."
                fi
                target="$architecture-unknown-linux-gnu"
                ;;
            *) fail "this operating system is $os. Use install.ps1 on Windows; this script is for macOS and Linux." ;;
        esac
    fi
    case "$target" in
        aarch64-apple-darwin | x86_64-apple-darwin | aarch64-unknown-linux-gnu | x86_64-unknown-linux-gnu) ;;
        *) fail "IBKIT_TARGET is $target. Use aarch64-apple-darwin, x86_64-apple-darwin, aarch64-unknown-linux-gnu or x86_64-unknown-linux-gnu." ;;
    esac
    case "$target" in
        *-apple-darwin) agent_name="libibkit_gateway_agent.dylib" ;;
        *) agent_name="libibkit_gateway_agent.so" ;;
    esac
}

download_file() {
    case "$1" in
        file://*) cp "${1#file://}" "$2" ;;
        https://*)
            if command_exists curl; then
                curl --proto '=https' --tlsv1.2 -fsSL "$1" -o "$2"
            elif command_exists wget; then
                wget --https-only --secure-protocol=TLSv1_2 -q "$1" -O "$2"
            else
                fail "neither curl nor wget is installed. Install one of them and run the installer again."
            fi
            ;;
        *) fail "IBKIT_RELEASES_URL is $RELEASES_URL. Use an https:// or file:// URL." ;;
    esac
}

download_release() {
    if github_cli_ready; then
        gh release download "$tag" --repo "$REPOSITORY" --pattern "$archive_name" \
            --pattern "$checksum_name" --dir "$temporary_directory" --clobber >/dev/null ||
            fail "gh could not download $archive_name from release $tag. Check that the release exists."
        return
    fi
    for name in "$archive_name" "$checksum_name"; do
        download_file "$RELEASES_URL/download/$tag/$name" "$temporary_directory/$name" ||
            fail "could not download $name from release $tag. Check that the release exists for $target."
    done
}

verify_archive() {
    expected="$(awk 'NF { print $1; exit }' "$temporary_directory/$checksum_name")"
    printf '%s\n' "$expected" | grep -Eq '^[0-9A-Fa-f]{64}$' ||
        fail "$checksum_name holds no SHA-256 checksum. Download the release again."
    if command_exists sha256sum; then
        actual="$(sha256sum "$temporary_directory/$archive_name" | awk '{ print $1 }')"
    elif command_exists shasum; then
        actual="$(shasum -a 256 "$temporary_directory/$archive_name" | awk '{ print $1 }')"
    else
        fail "neither sha256sum nor shasum is installed, and the installer checks the download with one. Install coreutils."
    fi
    [ "$(printf '%s' "$actual" | tr 'A-F' 'a-f')" = "$(printf '%s' "$expected" | tr 'A-F' 'a-f')" ] ||
        fail "the checksum of $archive_name does not match $checksum_name. Nothing was installed; download again."
}

# shellcheck disable=SC2016 # Variables expand when the shell reads its startup file.
configure_path() {
    case ":${PATH:-}:" in
        *":$install_directory:"*) return ;;
    esac
    # Only the default location is written into startup files, so no
    # user-supplied path ends up in shell code.
    if [ -n "${IBKIT_INSTALL_DIR:-}" ] || [ "${IBKIT_NO_MODIFY_PATH:-}" = 1 ]; then
        say "Add $install_directory to PATH to run ibkit from any directory."
        return
    fi
    line='case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac'
    case "${SHELL##*/}" in
        bash)
            add_path_line "${HOME}/.bashrc" "$line"
            if [ -f "${HOME}/.bash_profile" ]; then
                add_path_line "${HOME}/.bash_profile" "$line"
            elif [ -f "${HOME}/.bash_login" ]; then
                add_path_line "${HOME}/.bash_login" "$line"
            else
                add_path_line "${HOME}/.profile" "$line"
            fi
            ;;
        zsh) add_path_line "${ZDOTDIR:-$HOME}/.zshrc" "$line" ;;
        fish) add_path_line "${XDG_CONFIG_HOME:-$HOME/.config}/fish/config.fish" 'contains -- "$HOME/.local/bin" $PATH; or set -gx PATH "$HOME/.local/bin" $PATH' ;;
        sh | dash | ksh | ash) add_path_line "${HOME}/.profile" "$line" ;;
        *)
            say "Add $install_directory to your shell's PATH to run ibkit from any directory."
            return
            ;;
    esac
    say "Open a new terminal to use ibkit, or run $install_directory/ibkit now."
}

add_path_line() {
    if [ -f "$1" ] && grep -Fqx "$2" "$1"; then
        return
    fi
    mkdir -p "$(dirname "$1")"
    printf '\n%s\n' "$2" >>"$1"
    say "Added $install_directory to PATH in $1"
}

# Move a file into place through a staged copy in the same directory, so a
# running ibkit or Gateway keeps the file it opened.
place() {
    staged="$(mktemp "$2/.ibkit.install.XXXXXX")" ||
        fail "could not write to $2. Correct its permissions, or set IBKIT_INSTALL_DIR to a writable directory."
    cp "$1" "$staged"
    chmod "$3" "$staged"
    mv -f "$staged" "$2/$4"
    staged=''
}

install_release() {
    command_exists tar || fail "tar is not installed, and it unpacks $archive_name. Install tar."
    tar -xzf "$temporary_directory/$archive_name" -C "$temporary_directory"
    unpacked="$temporary_directory/ibkit-cli-$target"
    [ -f "$unpacked/ibkit" ] && [ -f "$unpacked/$agent_name" ] ||
        fail "$archive_name does not hold ibkit and $agent_name. Report this release at https://github.com/$REPOSITORY/issues."
    probe="$("$unpacked/ibkit" --version 2>&1)" ||
        fail "the ibkit $version binary cannot run on this system ($probe). Update the system (glibc 2.35 or newer on Linux), or use the container image."

    install_directory="${IBKIT_INSTALL_DIR:-${HOME:?HOME is not set}/.local/bin}"
    mkdir -p "$install_directory"
    install_directory="$(cd "$install_directory" && pwd)"
    # find_agent looks next to the binary and in ../lib.
    lib_directory="$(dirname "$install_directory")/lib"
    mkdir -p "$lib_directory"
    # The agent first: the new binary needs the new agent.
    place "$unpacked/$agent_name" "$lib_directory" 0644 "$agent_name"
    place "$unpacked/ibkit" "$install_directory" 0755 ibkit
    say "Installed ibkit $version to $install_directory/ibkit and its agent to $lib_directory/$agent_name"
    configure_path
}

umask 077
staged=''
temporary_directory="$(mktemp -d "${TMPDIR:-/tmp}/ibkit-install.XXXXXX")" ||
    fail "could not create a temporary directory. Set TMPDIR to a writable directory."
cleanup() {
    rm -rf "$temporary_directory"
    [ -z "$staged" ] || rm -f "$staged"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

resolve_tag
detect_target
archive_name="ibkit-cli-$target.tar.gz"
checksum_name="$archive_name.sha256"
say "Installing ibkit $version for $target"
download_release
verify_archive
install_release
