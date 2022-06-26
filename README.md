# git-snapshot

[![Rust](https://github.com/rylio/git-snapshot/actions/workflows/rust.yml/badge.svg)](https://github.com/rylio/git-snapshot/actions/workflows/rust.yml)
![crates.io](https://img.shields.io/crates/v/git-snapshot.svg)

## Install

`cargo install git-snapshot`

## Usage

#### Snapshot current branch

`git snapshot`

#### Enable pushing snapshots to a remote

`git config remote.<YOUR_REMOTE_NAME>.snapshotenabled true`

#### Add repo to watcher

`git snapshot watch .`
