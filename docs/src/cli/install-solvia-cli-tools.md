---
title: Install the Solvia Tool Suite
---

There are multiple ways to install the Solvia tools on your computer
depending on your preferred workflow:

- [Use Solvia's Install Tool (Simplest option)](#use-solvias-install-tool)
- [Download Prebuilt Binaries](#download-prebuilt-binaries)
- [Build from Source](#build-from-source)

## Use Solvia's Install Tool

### MacOS & Linux

- Open your favorite Terminal application

- Install the Solvia release
  [LATEST_SOLVIA_RELEASE_VERSION](https://github.com/solvia-labs/solvia/releases/tag/LATEST_SOLVIA_RELEASE_VERSION) on your
  machine by running:

```bash
sh -c "$(curl -sSfL https://release.solvia.com/LATEST_SOLVIA_RELEASE_VERSION/install)"
```

- You can replace `LATEST_SOLVIA_RELEASE_VERSION` with the release tag matching
  the software version of your desired release, or use one of the three symbolic
  channel names: `stable`, `beta`, or `edge`.

- The following output indicates a successful update:

```text
downloading LATEST_SOLVIA_RELEASE_VERSION installer
Configuration: /home/solvia/.config/solvia/install/config.yml
Active release directory: /home/solvia/.local/share/solvia/install/active_release
* Release version: LATEST_SOLVIA_RELEASE_VERSION
* Release URL: https://github.com/solvia-labs/solvia/releases/download/LATEST_SOLVIA_RELEASE_VERSION/solvia-release-x86_64-unknown-linux-gnu.tar.bz2
Update successful
```

- Depending on your system, the end of the installer messaging may prompt you
  to

```bash
Please update your PATH environment variable to include the solvia programs:
```

- If you get the above message, copy and paste the recommended command below
  it to update `PATH`
- Confirm you have the desired version of `solvia` installed by running:

```bash
solvia --version
```

- After a successful install, `solvia-install update` may be used to easily
  update the Solvia software to a newer version at any time.

---

### Windows

- Open a Command Prompt (`cmd.exe`) as an Administrator

  - Search for Command Prompt in the Windows search bar. When the Command
    Prompt app appears, right-click and select “Open as Administrator”.
    If you are prompted by a pop-up window asking “Do you want to allow this app to
    make changes to your device?”, click Yes.

- Copy and paste the following command, then press Enter to download the Solvia
  installer into a temporary directory:

```bash
curl https://release.solvia.com/LATEST_SOLVIA_RELEASE_VERSION/solvia-install-init-x86_64-pc-windows-msvc.exe --output C:\solvia-install-tmp\solvia-install-init.exe --create-dirs
```

- Copy and paste the following command, then press Enter to install the latest
  version of Solvia. If you see a security pop-up by your system, please select
  to allow the program to run.

```bash
C:\solvia-install-tmp\solvia-install-init.exe LATEST_SOLVIA_RELEASE_VERSION
```

- When the installer is finished, press Enter.

- Close the command prompt window and re-open a new command prompt window as a
  normal user
  - Search for "Command Prompt" in the search bar, then left click on the
    Command Prompt app icon, no need to run as Administrator)
- Confirm you have the desired version of `solvia` installed by entering:

```bash
solvia --version
```

- After a successful install, `solvia-install update` may be used to easily
  update the Solvia software to a newer version at any time.

## Download Prebuilt Binaries

If you would rather not use `solvia-install` to manage the install, you can
manually download and install the binaries.

### Linux

Download the binaries by navigating to
[https://github.com/solvia-labs/solvia/releases/latest](https://github.com/solvia-labs/solvia/releases/latest),
download **solvia-release-x86_64-unknown-linux-msvc.tar.bz2**, then extract the
archive:

```bash
tar jxf solvia-release-x86_64-unknown-linux-gnu.tar.bz2
cd solvia-release/
export PATH=$PWD/bin:$PATH
```

### MacOS

Download the binaries by navigating to
[https://github.com/solvia-labs/solvia/releases/latest](https://github.com/solvia-labs/solvia/releases/latest),
download **solvia-release-x86_64-apple-darwin.tar.bz2**, then extract the
archive:

```bash
tar jxf solvia-release-x86_64-apple-darwin.tar.bz2
cd solvia-release/
export PATH=$PWD/bin:$PATH
```

### Windows

- Download the binaries by navigating to
  [https://github.com/solvia-labs/solvia/releases/latest](https://github.com/solvia-labs/solvia/releases/latest),
  download **solvia-release-x86_64-pc-windows-msvc.tar.bz2**, then extract the
  archive using WinZip or similar.

- Open a Command Prompt and navigate to the directory into which you extracted
  the binaries and run:

```bash
cd solvia-release/
set PATH=%cd%/bin;%PATH%
```

## Build From Source

If you are unable to use the prebuilt binaries or prefer to build it yourself
from source, navigate to
[https://github.com/solvia-labs/solvia/releases/latest](https://github.com/solvia-labs/solvia/releases/latest),
and download the **Source Code** archive. Extract the code and build the
binaries with:

```bash
./scripts/cargo-install-all.sh .
export PATH=$PWD/bin:$PATH
```

You can then run the following command to obtain the same result as with
prebuilt binaries:

```bash
solvia-install init
```
