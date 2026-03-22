# Waffler CLI

The official developer CLI for the [Waffler](https://waffler.sywapps.com) platform. Build, validate, pack, and publish Waffler packages — and manage your developer namespace — all from the terminal.

## Installation

**Linux / macOS**
```sh
curl -fsSL https://raw.githubusercontent.com/SYW-Apps/waffler-sdk-cli/main/install.sh | sh
```

**Windows (PowerShell)**
```powershell
irm https://raw.githubusercontent.com/SYW-Apps/waffler-sdk-cli/main/install.ps1 | iex
```

**Cargo**
```sh
cargo install --git https://github.com/SYW-Apps/waffler-sdk-cli
```

Pre-built binaries for all platforms are available on the [Releases](https://github.com/SYW-Apps/waffler-sdk-cli/releases) page.

---

## Quick start

```sh
waffler login                        # authenticate with your Waffler account
waffler namespace claim my_tag       # claim a developer namespace tag
waffler scaffold                     # create a new package interactively
waffler validate                     # validate the package manifest
waffler publish                      # build, pack, and publish to the registry
```

---

## Commands

| Command | Description |
|---------|-------------|
| `waffler login` | Authenticate via browser (OAuth2 + PKCE) |
| `waffler logout` | Clear stored credentials |
| `waffler whoami` | Show the currently authenticated developer |
| `waffler scaffold` | Interactive wizard to create a new package |
| `waffler build` | Build the package (Rust, Node, Python) |
| `waffler validate` | Validate `package.json` and namespace structure |
| `waffler pack` | Build and archive into a distributable ZIP |
| `waffler publish` | Pack and upload to the Waffler registry |
| `waffler namespace` | Manage your developer namespace tags |
| `waffler org` | Manage organisations and members |
| `waffler update` | Update the CLI to the latest version |

### Namespace

```sh
waffler namespace claim <tag>        # claim a tag (e.g. "acme")
waffler namespace list               # list your claimed tags
waffler namespace check <tag>        # check if a tag is available
waffler namespace release <tag>      # release a tag (packages must be removed first)
```

### Org

```sh
waffler org create <id>              # create an org and claim its tag
waffler org invite <org> <email>     # invite a developer to your org
waffler org accept <token>           # accept an org invite
waffler org members <org>            # list org members
```

### Update

```sh
waffler update                       # check and install latest version
waffler update --check               # check only, don't download
```

---

## Package manifest (`package.json`)

Every Waffler package has a `package.json` at its root:

```json
{
  "namespace": "acme.my_package",
  "version": "1.0.0",
  "display_name": "My Package",
  "description": "What this package does",
  "visibility": "public",
  "features": {
    "native_module": true
  },
  "module": {
    "runtime": "wasm",
    "module_path": "my_package.wasm"
  }
}
```

The root segment of the namespace (e.g. `acme`) must be a tag you own. Run `waffler namespace claim acme` first.

---

## Development

**Requirements:** Rust 1.82+

```sh
git clone https://github.com/SYW-Apps/waffler-sdk-cli
cd waffler-sdk-cli
cargo build
cargo test
```

### Branch strategy

| Branch | Purpose |
|--------|---------|
| `main` | Stable — releases are tagged from here |
| `dev` | Integration — PRs merge here first |
| `feature/*` | Feature branches off `dev` |

CI runs on every push to `main` and `dev`, and on all pull requests. Releases are triggered by pushing a version tag to `main`:

```sh
git tag v0.2.0
git push origin v0.2.0
```

---

## Publishing a package

1. **Authenticate:** `waffler login`
2. **Claim a namespace tag:** `waffler namespace claim my_tag`
3. **Scaffold or write your package** with a valid `package.json`
4. **Publish:** `waffler publish`

The registry is at [registry.waffler.sywapps.com](https://registry.waffler.sywapps.com).

---

## License

Proprietary — © SYW Apps. All rights reserved.
