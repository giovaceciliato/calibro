# Calibro — Beginner's Guide

This guide assumes you have never used GitHub or programming tools. Follow it top to bottom.
Every unfamiliar word is explained the first time it appears.

---

## 0. Words you will meet

| Word | Plain-English meaning |
|---|---|
| **Source code** | The text files that describe the program (the `.rs` files). |
| **Build / compile** | Turning source code into a program your computer can run. Done by a tool called **Cargo**. |
| **Rust** | The programming language Calibro is written in. You install it once. |
| **Terminal** | A window where you type commands instead of clicking. On Windows it's called *PowerShell* or *Terminal*; on macOS *Terminal*; on Linux *Terminal* or *Konsole*. |
| **Repository ("repo")** | A project folder stored on GitHub, with its full history of changes. |
| **Commit** | A saved snapshot of changes in a repository, with a short message. |
| **Push** | Sending your commits from your computer to GitHub. |
| **Branch** | A line of history. Yours is called `main`. You can ignore branches for now. |
| **GitHub Actions** | Robots on GitHub that build and test the code every time you change it. |
| **GitHub Pages** | Free website hosting from GitHub. Calibro's browser version is published there. |
| **WebAssembly (wasm)** | A way to run compiled programs (like Calibro) inside a web browser. |

---

## 1. Which device do I need?

| Task | iPhone / iPad | Windows, Mac or Linux computer |
|---|---|---|
| Create a GitHub account, create the repository | ✅ | ✅ |
| Upload the project | ⚠️ awkward (Files app + browser, many files) | ✅ easy |
| Turn on GitHub Pages, watch builds | ✅ | ✅ |
| **Use the browser version** once published | ✅ (iPad works best; iPhone screen is small) | ✅ |
| Build and run the **desktop** app | ❌ | ✅ |
| Change code, fix errors, develop further | ❌ not practical | ✅ |

**You need a computer for real development.** A phone is enough to manage GitHub and to try
the published web version, but not to build or debug the app. Any recent laptop works; the
first build needs about 4 GB of free RAM, ~6 GB of disk and 10–25 minutes.

---

## 2. Get the project onto your computer

1. Download **`calibro.zip`** from the chat (click the file card → Download).
2. Find it in your *Downloads* folder.
3. Extract it:
   - **Windows:** right-click → *Extract All…* → *Extract*.
   - **macOS:** double-click the zip.
   - **Linux:** right-click → *Extract Here* (or `unzip calibro.zip`).
4. You now have a folder **`calibro`** containing `Cargo.toml`, `README.md`, `app`, `crates`,
   `docs`, `assets` and a **hidden** folder `.github`.
   - The `.github` folder holds the robots' instructions. It's hidden because its name starts
     with a dot. Show hidden files: **macOS** Finder → press `Cmd + Shift + .`;
     **Windows** File Explorer → *View* → *Show* → *Hidden items*; **Linux** → `Ctrl + H`.

---

## 3. Create a GitHub account (and, for Method B, an empty repository)

> Using **Method A (GitHub Desktop)** in section 4? Then only do step 1 here — GitHub
> Desktop creates the repository for you. Using **Method B or C**? Do all steps.

1. Open **https://github.com** in your browser → *Sign up* (skip if you have an account) → follow the steps → sign in.
2. Top right, click **+** → **New repository**.
3. **Repository name:** `calibro` (lowercase, no spaces; this becomes part of your web address).
4. **Visibility:** *Public* (GitHub Pages is free for public repositories; *Private* needs a paid plan for Pages).
5. **Do not** tick "Add a README", ".gitignore" or "license" — the project already has them.
6. Click **Create repository**. Keep this page open.

---

## 4. Upload the project (choose ONE method)

### Method A — GitHub Desktop (recommended for beginners)

GitHub Desktop is a free app with buttons instead of commands. It handles hidden folders and
many files reliably.

1. Install it from **https://desktop.github.com** and sign in with your GitHub account.
2. *File* → **Add local repository…** → choose the extracted `calibro` folder.
   It will say "this directory does not appear to be a Git repository" → click
   **create a repository** → leave the defaults → **Create repository**.
3. Click **Publish repository** (top bar). Name: `calibro`. **Untick** "Keep this code
   private" → **Publish repository**. (If it says the name already exists, you created an empty
   repository earlier: delete it on github.com — its *Settings* → bottom → *Delete this
   repository* — and publish again.)
4. Done. Open the repository on github.com: you should see the folders and the README.

### Method B — the GitHub website (no extra software)

1. On your empty repository page click **uploading an existing file**.
2. Open the extracted `calibro` folder, **select everything inside it** (including the
   hidden `.github` folder — see step 2.4), and drag it into the browser window.
   (The browser accepts up to 100 files at once; Calibro has fewer.)
3. Wait until all files are listed, type a message such as `First upload`, and click
   **Commit changes**.
4. Check that **`.github/workflows`** appears in the file list. If it doesn't, the hidden
   folder was skipped: click *Add file* → *Upload files* and drag the `.github` folder alone.

### Method C — Git on the command line (for later)

```bash
cd path/to/calibro          # go into the project folder
git init                    # turn it into a Git repository
git add .                   # stage all files
git commit -m "First upload"
git branch -M main
git remote add origin https://github.com/YOUR-USERNAME/calibro.git
git push -u origin main     # send it to GitHub (you'll be asked to sign in)
```

**How to confirm it worked:** your repository page shows `app`, `crates`, `docs`, `.github`,
`Cargo.toml`, `README.md`, and the README text appears below the file list.

---

## 5. Publish the browser version (GitHub Pages)

1. In your repository: **Settings** → **Pages** (left menu).
2. Under **Build and deployment → Source**, choose **GitHub Actions**.
3. Click the **Actions** tab at the top of the repository. You'll see three workflows:
   - **CI** — checks and tests the code.
   - **Deploy web app to GitHub Pages** — builds and publishes the website.
   - **Release desktop apps** — only runs when you make a release (section 9).
4. If nothing is running, open *Deploy web app to GitHub Pages* → **Run workflow** → **Run workflow**.
5. Wait (first time ~10–15 minutes). A green ✓ means success.
6. Your app is at **`https://YOUR-USERNAME.github.io/calibro/`** (also shown in *Settings → Pages*).

> **Important honesty note:** the browser build could not be compiled in the environment
> where Calibro was generated (it could not download the WebAssembly toolchain). The desktop
> build was compiled and tested. If the Pages or CI "web" job shows a red ✗, open it, copy the
> red error text, and paste it into a new chat — it is usually a small fix.

---

## 6. Install the tools to run Calibro on your computer

You only do this once.

### Windows
1. Download **rustup-init.exe** (64-bit) from **https://www.rust-lang.org/tools/install** and run it.
2. If it asks to install **Visual Studio C++ Build Tools**, say yes and accept the defaults
   (this is the "linker" Rust needs; ~2–5 GB).
3. Press Enter to choose the default installation.
4. Close and reopen your terminal (*Start* → type **PowerShell** → open).

### macOS
1. Open **Terminal** (Spotlight → "Terminal").
2. Install Apple's command-line tools: `xcode-select --install` (click *Install*).
3. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` → press Enter.
4. Close and reopen Terminal.

### Linux (Ubuntu, Fedora, Arch…)
1. Make sure a C compiler exists: Ubuntu/Debian `sudo apt install build-essential`;
   Fedora `sudo dnf install gcc`; Arch `sudo pacman -S base-devel`.
2. `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` → Enter.
3. Close and reopen the terminal.

**Immutable distributions (Bazzite, Fedora Silverblue/Kinoite, SteamOS):** the base system is
read-only, so install the compiler inside a *toolbox* container:
`toolbox create` → `toolbox enter` → `sudo dnf install gcc` → then the Rust command above.
Build and run Calibro from inside the toolbox; graphics work normally from there.

**Check it worked** (any system): type `cargo --version`. You should see something like
`cargo 1.9x.x`. If you see "not recognized" / "command not found", see Troubleshooting.

---

## 7. Build and run the desktop app

1. **Open a terminal in the project folder:**
   - Windows: open the `calibro` folder in File Explorer, click the address bar, type `powershell`, press Enter.
   - macOS: Terminal → type `cd ` (with a space), drag the `calibro` folder onto the window, press Enter.
   - Linux: right-click inside the folder → *Open in Terminal*.
2. Type and press Enter:
   ```
   cargo run --release -p calibro
   ```
3. The first time, Cargo downloads and compiles ~400 libraries. You'll see many
   `Compiling …` lines. **10–25 minutes is normal.** Later runs start in seconds.
4. Success looks like `Finished release [optimized] target(s)` followed by the Calibro window.
5. The built program is saved as `target/release/calibro` (`calibro.exe` on Windows); you can
   run that file directly next time.

Optional checks: `cargo test --workspace` runs all automated tests (68 tests; every line should end in `ok`).

### Run the browser version on your own computer (optional)
```
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
cd app
trunk serve
```
Then open **http://127.0.0.1:8080** in Chrome, Edge, Safari or Firefox.

---

## 8. Your first model (2 minutes)

1. Press **S** (or click *New sketch*) and choose **Top**.
2. Press **R** and click two corners → a rectangle. Press **C**, click a centre inside the
   rectangle, click again on the circumference → a circle. You can also press at the
   centre, drag to the desired radius and release. For an exact circle: click the centre,
   enter **Radius** in **Properties**, then click **Create circle**. **Esc** cancels the
   unfinished circle; switching tools discards unfinished points from the previous tool.
3. Click **Finish sketch**.
4. Click inside the rectangle (not the circle) — the region turns blue. Press **E**.
5. Drag the **blue arrow** up, or type a distance in the inspector on the right.
6. Click the top face → **Sketch on this face** → draw a circle → *Finish* → click it → **E**:
   a boss is joined onto the plate.
7. In **History** (right), click the first *Extrude* and change its distance — everything after
   it updates. **Ctrl+Z** undoes.
8. **Ctrl+S** saves a `.calibro` project. *File → Export* creates STEP, GLB, OBJ or STL files.

Mouse: right-drag = orbit, middle-drag (or Shift+right-drag) = pan, wheel = zoom.
All shortcuts: *Help → Keyboard shortcuts*.

---

## 9. Update the project later

### Small edits on the GitHub website
Open a file on github.com → pencil icon ✎ → edit → **Commit changes**. The robots rebuild
and republish automatically (watch the *Actions* tab).

### Replace or add files from your computer
- **GitHub Desktop:** change/copy files inside your local `calibro` folder → GitHub Desktop
  lists them → write a summary → **Commit to main** → **Push origin**.
- **Website:** *Add file* → *Upload files* → drag the new/changed files (same folder
  structure) → **Commit changes**. Uploading a file with the same name replaces it.

Every push to `main` re-runs **CI** and re-publishes the **web app** automatically.

### Make a downloadable desktop release
1. On github.com: right side **Releases** → **Create a new release**.
2. *Choose a tag* → enter a new version such as `v0.1.1` → *Create new tag* targeting the latest `main` commit. Use a new tag after the release-workflow changes have been merged.
3. **Publish release**. The *Release desktop apps* workflow builds Windows, macOS and Linux
   versions. Once all three builds succeed, it attaches all six desktop downloads and
   `SHA256SUMS.txt` to **Assets**, and adds direct download links to the release description.
   Windows gets a directly downloadable `.exe` plus a ZIP; Fedora gets an RPM; Linux gets
   a portable archive with `install.sh` (adds an app-menu launcher), plus a binary archive;
   macOS gets an Apple Silicon archive. Source code ZIP/TAR downloads are not the apps.
   If a build fails, open **Actions → Release desktop apps** and inspect/re-run the failed
   job. Packages remain available as workflow artifacts, but release upload waits for all
   platforms. Publishing a release or pushing a numeric/`v*` tag triggers the workflow; a manual
   workflow run on a regular branch only builds downloadable workflow artifacts.
   Note: the apps are not code-signed; Windows SmartScreen/macOS Gatekeeper will warn the
   first time (*More info → Run anyway* on Windows; right-click → *Open* on macOS).


### Recover downloads after a failed upload
If the platform builds succeeded but the final upload failed, you can reuse their packages:

1. Merge the release-recovery fix into `main` first. Re-running an old failed job uses its
   old workflow/tagged script and does not pick up the fix.
2. Open **Actions → Recover release downloads → Run workflow**, with branch **main**.
3. Enter the existing release tag exactly as published (`0.1.1` for the failed release)
   and the original release workflow run ID (`39946759282` for that run).
4. Run it. The recovery checks that the release exists, the original build commit matches
   its tag, and all three unexpired platform artifacts exist. It then attaches the six
   desktop downloads plus checksums and updates the download links, preserving prerelease
   status. It does not rebuild the apps or require a new tag/release.

Tags with or without a leading `v` are supported. Numeric and `v*` tag pushes trigger the
regular build workflow; publishing a GitHub Release also triggers it.

---

## 10. Troubleshooting

| Problem (what you see) | What it means | What to do |
|---|---|---|
| `cargo` / `rustc` "is not recognized" or "command not found" | The terminal doesn't know where Rust was installed yet | Close **all** terminals and open a new one. Still failing: log out and back in. Windows: rerun `rustup-init.exe`. |
| `linker 'cc' not found` / `link.exe not found` | The C linker Rust relies on is missing | Windows: install *Visual Studio C++ Build Tools*. macOS: `xcode-select --install`. Linux: install `build-essential`/`gcc` (Bazzite: inside a toolbox). |
| A dependency fails to download | Network/proxy hiccup | Run the same command again. Check your internet connection. |
| The app fails to compile | Code or toolchain problem | Run `rustup update` and try again. If still red, copy the **first** `error[...]` block and ask for help. |
| "No such file" / a required file is missing | The upload or extraction skipped files | Re-extract the zip; check `.github`, `app`, `crates`, `Cargo.toml` exist (hidden files visible). |
| The window opens and closes / won't launch | Usually no compatible graphics driver | Update graphics drivers. On Linux install Vulkan drivers (`mesa-vulkan-drivers`). Run from a terminal to read the message. |
| Grey viewport with "3D graphics (wgpu) are not available" | No GPU API available | Update drivers; on very old machines try another computer or the web version. |
| Browser shows a blank page | WebAssembly or GPU not supported, or wrong path | Use a current Chrome/Edge/Safari/Firefox. Press F12 → *Console* for the error. Check you opened `…github.io/calibro/` with the trailing slash. |
| GitHub Pages deployment fails | Pages not set to *GitHub Actions*, or build error | *Settings → Pages → Source: GitHub Actions*. Open the failed run in *Actions* and read the red step. |
| Works locally, not online | Asset path/sub-folder issue | The Pages workflow sets `--public-url /<repository-name>/`. If you renamed the repository, re-run the workflow. |
| An imported file cannot be opened | Unsupported or damaged file | Supported: STEP, STL, OBJ, `.calibro`. Some STEP entities aren't supported yet; the message says what failed. |
| A saved project cannot be reopened | File damaged or from a newer Calibro | The message says which. A copy of the previous save is next to it as `name.calibro.bak` — rename it to `.calibro`. |
| A feature turns **red** in History | The kernel couldn't build it | Hover it for the reason. Most common: faces exactly on top of each other (move a body 0.1 mm) or a sphere in a boolean (not supported yet). |

---

## 11. Checklist

- [ ] Downloaded `calibro.zip` and extracted it (hidden `.github` folder visible)
- [ ] Created a GitHub account (and, for Method B/C, an empty **public** repository `calibro`)
- [ ] Uploaded the project (GitHub Desktop or website) and see `.github/workflows` online
- [ ] *Settings → Pages → Source: GitHub Actions*
- [ ] *Actions* tab: CI and Pages workflows are green (if red, send me the error)
- [ ] Opened `https://YOUR-USERNAME.github.io/calibro/`
- [ ] Installed Rust on a computer (`cargo --version` works)
- [ ] `cargo run --release -p calibro` opens the desktop app
- [ ] Made a first model, saved it, reopened it, exported a STEP/GLB
- [ ] (Optional) Created release `v0.1.0` to get downloadable desktop apps

---

## 12. Fedora and Bazzite

Two Linux packages are built for each release (and by `packaging/build-linux-packages.sh`):

| Package | For | How to install | How to remove |
|---|---|---|---|
| `calibro-<version>-1.x86_64.rpm` | Fedora Workstation 40+ (normal, not immutable) | double-click it (opens *Software*) → *Install*, or `sudo dnf install ./calibro-<version>-1.x86_64.rpm` | `sudo dnf remove calibro` |
| `calibro-<version>-linux-x86_64.tar.gz` | **Bazzite**, Silverblue, Kinoite, any Linux | extract it, open a terminal in the folder, run `./install.sh` (no password needed) | `./install.sh --remove` |

On Bazzite prefer the `.tar.gz`: installing an RPM there means layering it with
`rpm-ostree install ./calibro-….rpm` and rebooting, which works but is slower to update.
After installing, Calibro appears in the app menu, and `.calibro` files open with it on double-click.
