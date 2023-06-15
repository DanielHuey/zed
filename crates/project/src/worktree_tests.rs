use crate::{
    worktree::{Event, Snapshot, WorktreeHandle},
    EntryKind, PathChange, Worktree,
};
use anyhow::Result;
use client::Client;
use fs::{repository::GitFileStatus, FakeFs, Fs, RealFs, RemoveOptions};
use git::GITIGNORE;
use gpui::{executor::Deterministic, ModelContext, Task, TestAppContext};
use parking_lot::Mutex;
use pretty_assertions::assert_eq;
use rand::prelude::*;
use serde_json::json;
use std::{
    env,
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use util::{http::FakeHttpClient, test::temp_tree, ResultExt};

#[gpui::test]
async fn test_traversal(cx: &mut TestAppContext) {
    let fs = FakeFs::new(cx.background());
    fs.insert_tree(
        "/root",
        json!({
           ".gitignore": "a/b\n",
           "a": {
               "b": "",
               "c": "",
           }
        }),
    )
    .await;

    let http_client = FakeHttpClient::with_404_response();
    let client = cx.read(|cx| Client::new(http_client, cx));

    let tree = Worktree::local(
        client,
        Path::new("/root"),
        true,
        fs,
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();
    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;

    tree.read_with(cx, |tree, _| {
        assert_eq!(
            tree.entries(false)
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![
                Path::new(""),
                Path::new(".gitignore"),
                Path::new("a"),
                Path::new("a/c"),
            ]
        );
        assert_eq!(
            tree.entries(true)
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![
                Path::new(""),
                Path::new(".gitignore"),
                Path::new("a"),
                Path::new("a/b"),
                Path::new("a/c"),
            ]
        );
    })
}

#[gpui::test]
async fn test_descendent_entries(cx: &mut TestAppContext) {
    let fs = FakeFs::new(cx.background());
    fs.insert_tree(
        "/root",
        json!({
            "a": "",
            "b": {
               "c": {
                   "d": ""
               },
               "e": {}
            },
            "f": "",
            "g": {
                "h": {}
            },
            "i": {
                "j": {
                    "k": ""
                },
                "l": {

                }
            },
            ".gitignore": "i/j\n",
        }),
    )
    .await;

    let http_client = FakeHttpClient::with_404_response();
    let client = cx.read(|cx| Client::new(http_client, cx));

    let tree = Worktree::local(
        client,
        Path::new("/root"),
        true,
        fs,
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();
    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;

    tree.read_with(cx, |tree, _| {
        assert_eq!(
            tree.descendent_entries(false, false, Path::new("b"))
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![Path::new("b/c/d"),]
        );
        assert_eq!(
            tree.descendent_entries(true, false, Path::new("b"))
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![
                Path::new("b"),
                Path::new("b/c"),
                Path::new("b/c/d"),
                Path::new("b/e"),
            ]
        );

        assert_eq!(
            tree.descendent_entries(false, false, Path::new("g"))
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            tree.descendent_entries(true, false, Path::new("g"))
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![Path::new("g"), Path::new("g/h"),]
        );

        assert_eq!(
            tree.descendent_entries(false, false, Path::new("i"))
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            tree.descendent_entries(false, true, Path::new("i"))
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![Path::new("i/j/k")]
        );
        assert_eq!(
            tree.descendent_entries(true, false, Path::new("i"))
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![Path::new("i"), Path::new("i/l"),]
        );
    })
}

#[gpui::test(iterations = 10)]
async fn test_circular_symlinks(executor: Arc<Deterministic>, cx: &mut TestAppContext) {
    let fs = FakeFs::new(cx.background());
    fs.insert_tree(
        "/root",
        json!({
            "lib": {
                "a": {
                    "a.txt": ""
                },
                "b": {
                    "b.txt": ""
                }
            }
        }),
    )
    .await;
    fs.insert_symlink("/root/lib/a/lib", "..".into()).await;
    fs.insert_symlink("/root/lib/b/lib", "..".into()).await;

    let client = cx.read(|cx| Client::new(FakeHttpClient::with_404_response(), cx));
    let tree = Worktree::local(
        client,
        Path::new("/root"),
        true,
        fs.clone(),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();

    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;

    tree.read_with(cx, |tree, _| {
        assert_eq!(
            tree.entries(false)
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![
                Path::new(""),
                Path::new("lib"),
                Path::new("lib/a"),
                Path::new("lib/a/a.txt"),
                Path::new("lib/a/lib"),
                Path::new("lib/b"),
                Path::new("lib/b/b.txt"),
                Path::new("lib/b/lib"),
            ]
        );
    });

    fs.rename(
        Path::new("/root/lib/a/lib"),
        Path::new("/root/lib/a/lib-2"),
        Default::default(),
    )
    .await
    .unwrap();
    executor.run_until_parked();
    tree.read_with(cx, |tree, _| {
        assert_eq!(
            tree.entries(false)
                .map(|entry| entry.path.as_ref())
                .collect::<Vec<_>>(),
            vec![
                Path::new(""),
                Path::new("lib"),
                Path::new("lib/a"),
                Path::new("lib/a/a.txt"),
                Path::new("lib/a/lib-2"),
                Path::new("lib/b"),
                Path::new("lib/b/b.txt"),
                Path::new("lib/b/lib"),
            ]
        );
    });
}

#[gpui::test]
async fn test_rescan_with_gitignore(cx: &mut TestAppContext) {
    // .gitignores are handled explicitly by Zed and do not use the git
    // machinery that the git_tests module checks
    let parent_dir = temp_tree(json!({
        ".gitignore": "ancestor-ignored-file1\nancestor-ignored-file2\n",
        "tree": {
            ".git": {},
            ".gitignore": "ignored-dir\n",
            "tracked-dir": {
                "tracked-file1": "",
                "ancestor-ignored-file1": "",
            },
            "ignored-dir": {
                "ignored-file1": ""
            }
        }
    }));
    let dir = parent_dir.path().join("tree");

    let client = cx.read(|cx| Client::new(FakeHttpClient::with_404_response(), cx));

    let tree = Worktree::local(
        client,
        dir.as_path(),
        true,
        Arc::new(RealFs),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();
    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;
    tree.flush_fs_events(cx).await;
    cx.read(|cx| {
        let tree = tree.read(cx);
        assert!(
            !tree
                .entry_for_path("tracked-dir/tracked-file1")
                .unwrap()
                .is_ignored
        );
        assert!(
            tree.entry_for_path("tracked-dir/ancestor-ignored-file1")
                .unwrap()
                .is_ignored
        );
        assert!(
            tree.entry_for_path("ignored-dir/ignored-file1")
                .unwrap()
                .is_ignored
        );
    });

    std::fs::write(dir.join("tracked-dir/tracked-file2"), "").unwrap();
    std::fs::write(dir.join("tracked-dir/ancestor-ignored-file2"), "").unwrap();
    std::fs::write(dir.join("ignored-dir/ignored-file2"), "").unwrap();
    tree.flush_fs_events(cx).await;
    cx.read(|cx| {
        let tree = tree.read(cx);
        assert!(
            !tree
                .entry_for_path("tracked-dir/tracked-file2")
                .unwrap()
                .is_ignored
        );
        assert!(
            tree.entry_for_path("tracked-dir/ancestor-ignored-file2")
                .unwrap()
                .is_ignored
        );
        assert!(
            tree.entry_for_path("ignored-dir/ignored-file2")
                .unwrap()
                .is_ignored
        );
        assert!(tree.entry_for_path(".git").unwrap().is_ignored);
    });
}

#[gpui::test]
async fn test_write_file(cx: &mut TestAppContext) {
    let dir = temp_tree(json!({
        ".git": {},
        ".gitignore": "ignored-dir\n",
        "tracked-dir": {},
        "ignored-dir": {}
    }));

    let client = cx.read(|cx| Client::new(FakeHttpClient::with_404_response(), cx));

    let tree = Worktree::local(
        client,
        dir.path(),
        true,
        Arc::new(RealFs),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();
    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;
    tree.flush_fs_events(cx).await;

    tree.update(cx, |tree, cx| {
        tree.as_local().unwrap().write_file(
            Path::new("tracked-dir/file.txt"),
            "hello".into(),
            Default::default(),
            cx,
        )
    })
    .await
    .unwrap();
    tree.update(cx, |tree, cx| {
        tree.as_local().unwrap().write_file(
            Path::new("ignored-dir/file.txt"),
            "world".into(),
            Default::default(),
            cx,
        )
    })
    .await
    .unwrap();

    tree.read_with(cx, |tree, _| {
        let tracked = tree.entry_for_path("tracked-dir/file.txt").unwrap();
        let ignored = tree.entry_for_path("ignored-dir/file.txt").unwrap();
        assert!(!tracked.is_ignored);
        assert!(ignored.is_ignored);
    });
}

#[gpui::test(iterations = 30)]
async fn test_create_directory_during_initial_scan(cx: &mut TestAppContext) {
    let client = cx.read(|cx| Client::new(FakeHttpClient::with_404_response(), cx));

    let fs = FakeFs::new(cx.background());
    fs.insert_tree(
        "/root",
        json!({
            "b": {},
            "c": {},
            "d": {},
        }),
    )
    .await;

    let tree = Worktree::local(
        client,
        "/root".as_ref(),
        true,
        fs,
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();

    let snapshot1 = tree.update(cx, |tree, cx| {
        let tree = tree.as_local_mut().unwrap();
        let snapshot = Arc::new(Mutex::new(tree.snapshot()));
        let _ = tree.observe_updates(0, cx, {
            let snapshot = snapshot.clone();
            move |update| {
                snapshot.lock().apply_remote_update(update).unwrap();
                async { true }
            }
        });
        snapshot
    });

    let entry = tree
        .update(cx, |tree, cx| {
            tree.as_local_mut()
                .unwrap()
                .create_entry("a/e".as_ref(), true, cx)
        })
        .await
        .unwrap();
    assert!(entry.is_dir());

    cx.foreground().run_until_parked();
    tree.read_with(cx, |tree, _| {
        assert_eq!(tree.entry_for_path("a/e").unwrap().kind, EntryKind::Dir);
    });

    let snapshot2 = tree.update(cx, |tree, _| tree.as_local().unwrap().snapshot());
    assert_eq!(
        snapshot1.lock().entries(true).collect::<Vec<_>>(),
        snapshot2.entries(true).collect::<Vec<_>>()
    );
}

#[gpui::test(iterations = 100)]
async fn test_random_worktree_operations_during_initial_scan(
    cx: &mut TestAppContext,
    mut rng: StdRng,
) {
    let operations = env::var("OPERATIONS")
        .map(|o| o.parse().unwrap())
        .unwrap_or(5);
    let initial_entries = env::var("INITIAL_ENTRIES")
        .map(|o| o.parse().unwrap())
        .unwrap_or(20);

    let root_dir = Path::new("/test");
    let fs = FakeFs::new(cx.background()) as Arc<dyn Fs>;
    fs.as_fake().insert_tree(root_dir, json!({})).await;
    for _ in 0..initial_entries {
        randomly_mutate_fs(&fs, root_dir, 1.0, &mut rng).await;
    }
    log::info!("generated initial tree");

    let client = cx.read(|cx| Client::new(FakeHttpClient::with_404_response(), cx));
    let worktree = Worktree::local(
        client.clone(),
        root_dir,
        true,
        fs.clone(),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();

    let mut snapshots = vec![worktree.read_with(cx, |tree, _| tree.as_local().unwrap().snapshot())];
    let updates = Arc::new(Mutex::new(Vec::new()));
    worktree.update(cx, |tree, cx| {
        check_worktree_change_events(tree, cx);

        let _ = tree.as_local_mut().unwrap().observe_updates(0, cx, {
            let updates = updates.clone();
            move |update| {
                updates.lock().push(update);
                async { true }
            }
        });
    });

    for _ in 0..operations {
        worktree
            .update(cx, |worktree, cx| {
                randomly_mutate_worktree(worktree, &mut rng, cx)
            })
            .await
            .log_err();
        worktree.read_with(cx, |tree, _| {
            tree.as_local().unwrap().snapshot().check_invariants()
        });

        if rng.gen_bool(0.6) {
            snapshots.push(worktree.read_with(cx, |tree, _| tree.as_local().unwrap().snapshot()));
        }
    }

    worktree
        .update(cx, |tree, _| tree.as_local_mut().unwrap().scan_complete())
        .await;

    cx.foreground().run_until_parked();

    let final_snapshot = worktree.read_with(cx, |tree, _| {
        let tree = tree.as_local().unwrap();
        let snapshot = tree.snapshot();
        snapshot.check_invariants();
        snapshot
    });

    for (i, snapshot) in snapshots.into_iter().enumerate().rev() {
        let mut updated_snapshot = snapshot.clone();
        for update in updates.lock().iter() {
            if update.scan_id >= updated_snapshot.scan_id() as u64 {
                updated_snapshot
                    .apply_remote_update(update.clone())
                    .unwrap();
            }
        }

        assert_eq!(
            updated_snapshot.entries(true).collect::<Vec<_>>(),
            final_snapshot.entries(true).collect::<Vec<_>>(),
            "wrong updates after snapshot {i}: {snapshot:#?} {updates:#?}",
        );
    }
}

#[gpui::test(iterations = 100)]
async fn test_random_worktree_changes(cx: &mut TestAppContext, mut rng: StdRng) {
    let operations = env::var("OPERATIONS")
        .map(|o| o.parse().unwrap())
        .unwrap_or(40);
    let initial_entries = env::var("INITIAL_ENTRIES")
        .map(|o| o.parse().unwrap())
        .unwrap_or(20);

    let root_dir = Path::new("/test");
    let fs = FakeFs::new(cx.background()) as Arc<dyn Fs>;
    fs.as_fake().insert_tree(root_dir, json!({})).await;
    for _ in 0..initial_entries {
        randomly_mutate_fs(&fs, root_dir, 1.0, &mut rng).await;
    }
    log::info!("generated initial tree");

    let client = cx.read(|cx| Client::new(FakeHttpClient::with_404_response(), cx));
    let worktree = Worktree::local(
        client.clone(),
        root_dir,
        true,
        fs.clone(),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();

    let updates = Arc::new(Mutex::new(Vec::new()));
    worktree.update(cx, |tree, cx| {
        check_worktree_change_events(tree, cx);

        let _ = tree.as_local_mut().unwrap().observe_updates(0, cx, {
            let updates = updates.clone();
            move |update| {
                updates.lock().push(update);
                async { true }
            }
        });
    });

    worktree
        .update(cx, |tree, _| tree.as_local_mut().unwrap().scan_complete())
        .await;

    fs.as_fake().pause_events();
    let mut snapshots = Vec::new();
    let mut mutations_len = operations;
    while mutations_len > 1 {
        if rng.gen_bool(0.2) {
            worktree
                .update(cx, |worktree, cx| {
                    randomly_mutate_worktree(worktree, &mut rng, cx)
                })
                .await
                .log_err();
        } else {
            randomly_mutate_fs(&fs, root_dir, 1.0, &mut rng).await;
        }

        let buffered_event_count = fs.as_fake().buffered_event_count();
        if buffered_event_count > 0 && rng.gen_bool(0.3) {
            let len = rng.gen_range(0..=buffered_event_count);
            log::info!("flushing {} events", len);
            fs.as_fake().flush_events(len);
        } else {
            randomly_mutate_fs(&fs, root_dir, 0.6, &mut rng).await;
            mutations_len -= 1;
        }

        cx.foreground().run_until_parked();
        if rng.gen_bool(0.2) {
            log::info!("storing snapshot {}", snapshots.len());
            let snapshot = worktree.read_with(cx, |tree, _| tree.as_local().unwrap().snapshot());
            snapshots.push(snapshot);
        }
    }

    log::info!("quiescing");
    fs.as_fake().flush_events(usize::MAX);
    cx.foreground().run_until_parked();
    let snapshot = worktree.read_with(cx, |tree, _| tree.as_local().unwrap().snapshot());
    snapshot.check_invariants();

    {
        let new_worktree = Worktree::local(
            client.clone(),
            root_dir,
            true,
            fs.clone(),
            Default::default(),
            &mut cx.to_async(),
        )
        .await
        .unwrap();
        new_worktree
            .update(cx, |tree, _| tree.as_local_mut().unwrap().scan_complete())
            .await;
        let new_snapshot =
            new_worktree.read_with(cx, |tree, _| tree.as_local().unwrap().snapshot());
        assert_eq!(
            snapshot.entries_without_ids(true),
            new_snapshot.entries_without_ids(true)
        );
    }

    for (i, mut prev_snapshot) in snapshots.into_iter().enumerate().rev() {
        for update in updates.lock().iter() {
            if update.scan_id >= prev_snapshot.scan_id() as u64 {
                prev_snapshot.apply_remote_update(update.clone()).unwrap();
            }
        }

        assert_eq!(
            prev_snapshot.entries(true).collect::<Vec<_>>(),
            snapshot.entries(true).collect::<Vec<_>>(),
            "wrong updates after snapshot {i}: {updates:#?}",
        );
    }
}

// The worktree's `UpdatedEntries` event can be used to follow along with
// all changes to the worktree's snapshot.
fn check_worktree_change_events(tree: &mut Worktree, cx: &mut ModelContext<Worktree>) {
    let mut entries = tree.entries(true).cloned().collect::<Vec<_>>();
    cx.subscribe(&cx.handle(), move |tree, _, event, _| {
        if let Event::UpdatedEntries(changes) = event {
            for (path, _, change_type) in changes.iter() {
                let entry = tree.entry_for_path(&path).cloned();
                let ix = match entries.binary_search_by_key(&path, |e| &e.path) {
                    Ok(ix) | Err(ix) => ix,
                };
                match change_type {
                    PathChange::Loaded => entries.insert(ix, entry.unwrap()),
                    PathChange::Added => entries.insert(ix, entry.unwrap()),
                    PathChange::Removed => drop(entries.remove(ix)),
                    PathChange::Updated => {
                        let entry = entry.unwrap();
                        let existing_entry = entries.get_mut(ix).unwrap();
                        assert_eq!(existing_entry.path, entry.path);
                        *existing_entry = entry;
                    }
                    PathChange::AddedOrUpdated => {
                        let entry = entry.unwrap();
                        if entries.get(ix).map(|e| &e.path) == Some(&entry.path) {
                            *entries.get_mut(ix).unwrap() = entry;
                        } else {
                            entries.insert(ix, entry);
                        }
                    }
                }
            }

            let new_entries = tree.entries(true).cloned().collect::<Vec<_>>();
            assert_eq!(entries, new_entries, "incorrect changes: {:?}", changes);
        }
    })
    .detach();
}

fn randomly_mutate_worktree(
    worktree: &mut Worktree,
    rng: &mut impl Rng,
    cx: &mut ModelContext<Worktree>,
) -> Task<Result<()>> {
    log::info!("mutating worktree");
    let worktree = worktree.as_local_mut().unwrap();
    let snapshot = worktree.snapshot();
    let entry = snapshot.entries(false).choose(rng).unwrap();

    match rng.gen_range(0_u32..100) {
        0..=33 if entry.path.as_ref() != Path::new("") => {
            log::info!("deleting entry {:?} ({})", entry.path, entry.id.0);
            worktree.delete_entry(entry.id, cx).unwrap()
        }
        ..=66 if entry.path.as_ref() != Path::new("") => {
            let other_entry = snapshot.entries(false).choose(rng).unwrap();
            let new_parent_path = if other_entry.is_dir() {
                other_entry.path.clone()
            } else {
                other_entry.path.parent().unwrap().into()
            };
            let mut new_path = new_parent_path.join(random_filename(rng));
            if new_path.starts_with(&entry.path) {
                new_path = random_filename(rng).into();
            }

            log::info!(
                "renaming entry {:?} ({}) to {:?}",
                entry.path,
                entry.id.0,
                new_path
            );
            let task = worktree.rename_entry(entry.id, new_path, cx).unwrap();
            cx.foreground().spawn(async move {
                task.await?;
                Ok(())
            })
        }
        _ => {
            let task = if entry.is_dir() {
                let child_path = entry.path.join(random_filename(rng));
                let is_dir = rng.gen_bool(0.3);
                log::info!(
                    "creating {} at {:?}",
                    if is_dir { "dir" } else { "file" },
                    child_path,
                );
                worktree.create_entry(child_path, is_dir, cx)
            } else {
                log::info!("overwriting file {:?} ({})", entry.path, entry.id.0);
                worktree.write_file(entry.path.clone(), "".into(), Default::default(), cx)
            };
            cx.foreground().spawn(async move {
                task.await?;
                Ok(())
            })
        }
    }
}

async fn randomly_mutate_fs(
    fs: &Arc<dyn Fs>,
    root_path: &Path,
    insertion_probability: f64,
    rng: &mut impl Rng,
) {
    log::info!("mutating fs");
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for path in fs.as_fake().paths(false) {
        if path.starts_with(root_path) {
            if fs.is_file(&path).await {
                files.push(path);
            } else {
                dirs.push(path);
            }
        }
    }

    if (files.is_empty() && dirs.len() == 1) || rng.gen_bool(insertion_probability) {
        let path = dirs.choose(rng).unwrap();
        let new_path = path.join(random_filename(rng));

        if rng.gen() {
            log::info!(
                "creating dir {:?}",
                new_path.strip_prefix(root_path).unwrap()
            );
            fs.create_dir(&new_path).await.unwrap();
        } else {
            log::info!(
                "creating file {:?}",
                new_path.strip_prefix(root_path).unwrap()
            );
            fs.create_file(&new_path, Default::default()).await.unwrap();
        }
    } else if rng.gen_bool(0.05) {
        let ignore_dir_path = dirs.choose(rng).unwrap();
        let ignore_path = ignore_dir_path.join(&*GITIGNORE);

        let subdirs = dirs
            .iter()
            .filter(|d| d.starts_with(&ignore_dir_path))
            .cloned()
            .collect::<Vec<_>>();
        let subfiles = files
            .iter()
            .filter(|d| d.starts_with(&ignore_dir_path))
            .cloned()
            .collect::<Vec<_>>();
        let files_to_ignore = {
            let len = rng.gen_range(0..=subfiles.len());
            subfiles.choose_multiple(rng, len)
        };
        let dirs_to_ignore = {
            let len = rng.gen_range(0..subdirs.len());
            subdirs.choose_multiple(rng, len)
        };

        let mut ignore_contents = String::new();
        for path_to_ignore in files_to_ignore.chain(dirs_to_ignore) {
            writeln!(
                ignore_contents,
                "{}",
                path_to_ignore
                    .strip_prefix(&ignore_dir_path)
                    .unwrap()
                    .to_str()
                    .unwrap()
            )
            .unwrap();
        }
        log::info!(
            "creating gitignore {:?} with contents:\n{}",
            ignore_path.strip_prefix(&root_path).unwrap(),
            ignore_contents
        );
        fs.save(
            &ignore_path,
            &ignore_contents.as_str().into(),
            Default::default(),
        )
        .await
        .unwrap();
    } else {
        let old_path = {
            let file_path = files.choose(rng);
            let dir_path = dirs[1..].choose(rng);
            file_path.into_iter().chain(dir_path).choose(rng).unwrap()
        };

        let is_rename = rng.gen();
        if is_rename {
            let new_path_parent = dirs
                .iter()
                .filter(|d| !d.starts_with(old_path))
                .choose(rng)
                .unwrap();

            let overwrite_existing_dir =
                !old_path.starts_with(&new_path_parent) && rng.gen_bool(0.3);
            let new_path = if overwrite_existing_dir {
                fs.remove_dir(
                    &new_path_parent,
                    RemoveOptions {
                        recursive: true,
                        ignore_if_not_exists: true,
                    },
                )
                .await
                .unwrap();
                new_path_parent.to_path_buf()
            } else {
                new_path_parent.join(random_filename(rng))
            };

            log::info!(
                "renaming {:?} to {}{:?}",
                old_path.strip_prefix(&root_path).unwrap(),
                if overwrite_existing_dir {
                    "overwrite "
                } else {
                    ""
                },
                new_path.strip_prefix(&root_path).unwrap()
            );
            fs.rename(
                &old_path,
                &new_path,
                fs::RenameOptions {
                    overwrite: true,
                    ignore_if_exists: true,
                },
            )
            .await
            .unwrap();
        } else if fs.is_file(&old_path).await {
            log::info!(
                "deleting file {:?}",
                old_path.strip_prefix(&root_path).unwrap()
            );
            fs.remove_file(old_path, Default::default()).await.unwrap();
        } else {
            log::info!(
                "deleting dir {:?}",
                old_path.strip_prefix(&root_path).unwrap()
            );
            fs.remove_dir(
                &old_path,
                RemoveOptions {
                    recursive: true,
                    ignore_if_not_exists: true,
                },
            )
            .await
            .unwrap();
        }
    }
}

fn random_filename(rng: &mut impl Rng) -> String {
    (0..6)
        .map(|_| rng.sample(rand::distributions::Alphanumeric))
        .map(char::from)
        .collect()
}

#[gpui::test]
async fn test_rename_work_directory(cx: &mut TestAppContext) {
    let root = temp_tree(json!({
        "projects": {
            "project1": {
                "a": "",
                "b": "",
            }
        },

    }));
    let root_path = root.path();

    let http_client = FakeHttpClient::with_404_response();
    let client = cx.read(|cx| Client::new(http_client, cx));
    let tree = Worktree::local(
        client,
        root_path,
        true,
        Arc::new(RealFs),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();

    let repo = git_init(&root_path.join("projects/project1"));
    git_add("a", &repo);
    git_commit("init", &repo);
    std::fs::write(root_path.join("projects/project1/a"), "aa").ok();

    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;

    tree.flush_fs_events(cx).await;

    cx.read(|cx| {
        let tree = tree.read(cx);
        let (work_dir, _) = tree.repositories().next().unwrap();
        assert_eq!(work_dir.as_ref(), Path::new("projects/project1"));
        assert_eq!(
            tree.status_for_file(Path::new("projects/project1/a")),
            Some(GitFileStatus::Modified)
        );
        assert_eq!(
            tree.status_for_file(Path::new("projects/project1/b")),
            Some(GitFileStatus::Added)
        );
    });

    std::fs::rename(
        root_path.join("projects/project1"),
        root_path.join("projects/project2"),
    )
    .ok();
    tree.flush_fs_events(cx).await;

    cx.read(|cx| {
        let tree = tree.read(cx);
        let (work_dir, _) = tree.repositories().next().unwrap();
        assert_eq!(work_dir.as_ref(), Path::new("projects/project2"));
        assert_eq!(
            tree.status_for_file(Path::new("projects/project2/a")),
            Some(GitFileStatus::Modified)
        );
        assert_eq!(
            tree.status_for_file(Path::new("projects/project2/b")),
            Some(GitFileStatus::Added)
        );
    });
}

#[gpui::test]
async fn test_git_repository_for_path(cx: &mut TestAppContext) {
    let root = temp_tree(json!({
        "c.txt": "",
        "dir1": {
            ".git": {},
            "deps": {
                "dep1": {
                    ".git": {},
                    "src": {
                        "a.txt": ""
                    }
                }
            },
            "src": {
                "b.txt": ""
            }
        },
    }));

    let http_client = FakeHttpClient::with_404_response();
    let client = cx.read(|cx| Client::new(http_client, cx));
    let tree = Worktree::local(
        client,
        root.path(),
        true,
        Arc::new(RealFs),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();

    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;
    tree.flush_fs_events(cx).await;

    tree.read_with(cx, |tree, _cx| {
        let tree = tree.as_local().unwrap();

        assert!(tree.repository_for_path("c.txt".as_ref()).is_none());

        let entry = tree.repository_for_path("dir1/src/b.txt".as_ref()).unwrap();
        assert_eq!(
            entry
                .work_directory(tree)
                .map(|directory| directory.as_ref().to_owned()),
            Some(Path::new("dir1").to_owned())
        );

        let entry = tree
            .repository_for_path("dir1/deps/dep1/src/a.txt".as_ref())
            .unwrap();
        assert_eq!(
            entry
                .work_directory(tree)
                .map(|directory| directory.as_ref().to_owned()),
            Some(Path::new("dir1/deps/dep1").to_owned())
        );

        let entries = tree.files(false, 0);

        let paths_with_repos = tree
            .entries_with_repositories(entries)
            .map(|(entry, repo)| {
                (
                    entry.path.as_ref(),
                    repo.and_then(|repo| {
                        repo.work_directory(&tree)
                            .map(|work_directory| work_directory.0.to_path_buf())
                    }),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            paths_with_repos,
            &[
                (Path::new("c.txt"), None),
                (
                    Path::new("dir1/deps/dep1/src/a.txt"),
                    Some(Path::new("dir1/deps/dep1").into())
                ),
                (Path::new("dir1/src/b.txt"), Some(Path::new("dir1").into())),
            ]
        );
    });

    let repo_update_events = Arc::new(Mutex::new(vec![]));
    tree.update(cx, |_, cx| {
        let repo_update_events = repo_update_events.clone();
        cx.subscribe(&tree, move |_, _, event, _| {
            if let Event::UpdatedGitRepositories(update) = event {
                repo_update_events.lock().push(update.clone());
            }
        })
        .detach();
    });

    std::fs::write(root.path().join("dir1/.git/random_new_file"), "hello").unwrap();
    tree.flush_fs_events(cx).await;

    assert_eq!(
        repo_update_events.lock()[0]
            .iter()
            .map(|e| e.0.clone())
            .collect::<Vec<Arc<Path>>>(),
        vec![Path::new("dir1").into()]
    );

    std::fs::remove_dir_all(root.path().join("dir1/.git")).unwrap();
    tree.flush_fs_events(cx).await;

    tree.read_with(cx, |tree, _cx| {
        let tree = tree.as_local().unwrap();

        assert!(tree
            .repository_for_path("dir1/src/b.txt".as_ref())
            .is_none());
    });
}

#[gpui::test]
async fn test_git_status(deterministic: Arc<Deterministic>, cx: &mut TestAppContext) {
    const IGNORE_RULE: &'static str = "**/target";

    let root = temp_tree(json!({
        "project": {
            "a.txt": "a",
            "b.txt": "bb",
            "c": {
                "d": {
                    "e.txt": "eee"
                }
            },
            "f.txt": "ffff",
            "target": {
                "build_file": "???"
            },
            ".gitignore": IGNORE_RULE
        },

    }));

    let http_client = FakeHttpClient::with_404_response();
    let client = cx.read(|cx| Client::new(http_client, cx));
    let tree = Worktree::local(
        client,
        root.path(),
        true,
        Arc::new(RealFs),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();

    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;

    const A_TXT: &'static str = "a.txt";
    const B_TXT: &'static str = "b.txt";
    const E_TXT: &'static str = "c/d/e.txt";
    const F_TXT: &'static str = "f.txt";
    const DOTGITIGNORE: &'static str = ".gitignore";
    const BUILD_FILE: &'static str = "target/build_file";
    let project_path: &Path = &Path::new("project");

    let work_dir = root.path().join("project");
    let mut repo = git_init(work_dir.as_path());
    repo.add_ignore_rule(IGNORE_RULE).unwrap();
    git_add(Path::new(A_TXT), &repo);
    git_add(Path::new(E_TXT), &repo);
    git_add(Path::new(DOTGITIGNORE), &repo);
    git_commit("Initial commit", &repo);

    tree.flush_fs_events(cx).await;
    deterministic.run_until_parked();

    // Check that the right git state is observed on startup
    tree.read_with(cx, |tree, _cx| {
        let snapshot = tree.snapshot();
        assert_eq!(snapshot.repositories().count(), 1);
        let (dir, _) = snapshot.repositories().next().unwrap();
        assert_eq!(dir.as_ref(), Path::new("project"));

        assert_eq!(
            snapshot.status_for_file(project_path.join(B_TXT)),
            Some(GitFileStatus::Added)
        );
        assert_eq!(
            snapshot.status_for_file(project_path.join(F_TXT)),
            Some(GitFileStatus::Added)
        );
    });

    std::fs::write(work_dir.join(A_TXT), "aa").unwrap();

    tree.flush_fs_events(cx).await;
    deterministic.run_until_parked();

    tree.read_with(cx, |tree, _cx| {
        let snapshot = tree.snapshot();

        assert_eq!(
            snapshot.status_for_file(project_path.join(A_TXT)),
            Some(GitFileStatus::Modified)
        );
    });

    git_add(Path::new(A_TXT), &repo);
    git_add(Path::new(B_TXT), &repo);
    git_commit("Committing modified and added", &repo);
    tree.flush_fs_events(cx).await;
    deterministic.run_until_parked();

    // Check that repo only changes are tracked
    tree.read_with(cx, |tree, _cx| {
        let snapshot = tree.snapshot();

        assert_eq!(
            snapshot.status_for_file(project_path.join(F_TXT)),
            Some(GitFileStatus::Added)
        );

        assert_eq!(snapshot.status_for_file(project_path.join(B_TXT)), None);
        assert_eq!(snapshot.status_for_file(project_path.join(A_TXT)), None);
    });

    git_reset(0, &repo);
    git_remove_index(Path::new(B_TXT), &repo);
    git_stash(&mut repo);
    std::fs::write(work_dir.join(E_TXT), "eeee").unwrap();
    std::fs::write(work_dir.join(BUILD_FILE), "this should be ignored").unwrap();
    tree.flush_fs_events(cx).await;
    deterministic.run_until_parked();

    // Check that more complex repo changes are tracked
    tree.read_with(cx, |tree, _cx| {
        let snapshot = tree.snapshot();

        assert_eq!(snapshot.status_for_file(project_path.join(A_TXT)), None);
        assert_eq!(
            snapshot.status_for_file(project_path.join(B_TXT)),
            Some(GitFileStatus::Added)
        );
        assert_eq!(
            snapshot.status_for_file(project_path.join(E_TXT)),
            Some(GitFileStatus::Modified)
        );
    });

    std::fs::remove_file(work_dir.join(B_TXT)).unwrap();
    std::fs::remove_dir_all(work_dir.join("c")).unwrap();
    std::fs::write(
        work_dir.join(DOTGITIGNORE),
        [IGNORE_RULE, "f.txt"].join("\n"),
    )
    .unwrap();

    git_add(Path::new(DOTGITIGNORE), &repo);
    git_commit("Committing modified git ignore", &repo);

    tree.flush_fs_events(cx).await;
    deterministic.run_until_parked();

    let mut renamed_dir_name = "first_directory/second_directory";
    const RENAMED_FILE: &'static str = "rf.txt";

    std::fs::create_dir_all(work_dir.join(renamed_dir_name)).unwrap();
    std::fs::write(
        work_dir.join(renamed_dir_name).join(RENAMED_FILE),
        "new-contents",
    )
    .unwrap();

    tree.flush_fs_events(cx).await;
    deterministic.run_until_parked();

    tree.read_with(cx, |tree, _cx| {
        let snapshot = tree.snapshot();
        assert_eq!(
            snapshot.status_for_file(&project_path.join(renamed_dir_name).join(RENAMED_FILE)),
            Some(GitFileStatus::Added)
        );
    });

    renamed_dir_name = "new_first_directory/second_directory";

    std::fs::rename(
        work_dir.join("first_directory"),
        work_dir.join("new_first_directory"),
    )
    .unwrap();

    tree.flush_fs_events(cx).await;
    deterministic.run_until_parked();

    tree.read_with(cx, |tree, _cx| {
        let snapshot = tree.snapshot();

        assert_eq!(
            snapshot.status_for_file(
                project_path
                    .join(Path::new(renamed_dir_name))
                    .join(RENAMED_FILE)
            ),
            Some(GitFileStatus::Added)
        );
    });
}

#[gpui::test]
async fn test_propagate_git_statuses(cx: &mut TestAppContext) {
    let fs = FakeFs::new(cx.background());
    fs.insert_tree(
        "/root",
        json!({
            ".git": {},
            "a": {
                "b": {
                    "c1.txt": "",
                    "c2.txt": "",
                },
                "d": {
                    "e1.txt": "",
                    "e2.txt": "",
                    "e3.txt": "",
                }
            },
            "f": {
                "no-status.txt": ""
            },
            "g": {
                "h1.txt": "",
                "h2.txt": ""
            },

        }),
    )
    .await;

    fs.set_status_for_repo_via_git_operation(
        &Path::new("/root/.git"),
        &[
            (Path::new("a/b/c1.txt"), GitFileStatus::Added),
            (Path::new("a/d/e2.txt"), GitFileStatus::Modified),
            (Path::new("g/h2.txt"), GitFileStatus::Conflict),
        ],
    );

    let http_client = FakeHttpClient::with_404_response();
    let client = cx.read(|cx| Client::new(http_client, cx));
    let tree = Worktree::local(
        client,
        Path::new("/root"),
        true,
        fs.clone(),
        Default::default(),
        &mut cx.to_async(),
    )
    .await
    .unwrap();

    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;

    cx.foreground().run_until_parked();
    let snapshot = tree.read_with(cx, |tree, _| tree.snapshot());

    check_propagated_statuses(
        &snapshot,
        &[
            (Path::new(""), Some(GitFileStatus::Conflict)),
            (Path::new("a"), Some(GitFileStatus::Modified)),
            (Path::new("a/b"), Some(GitFileStatus::Added)),
            (Path::new("a/b/c1.txt"), Some(GitFileStatus::Added)),
            (Path::new("a/b/c2.txt"), None),
            (Path::new("a/d"), Some(GitFileStatus::Modified)),
            (Path::new("a/d/e2.txt"), Some(GitFileStatus::Modified)),
            (Path::new("f"), None),
            (Path::new("f/no-status.txt"), None),
            (Path::new("g"), Some(GitFileStatus::Conflict)),
            (Path::new("g/h2.txt"), Some(GitFileStatus::Conflict)),
        ],
    );

    check_propagated_statuses(
        &snapshot,
        &[
            (Path::new("a/b"), Some(GitFileStatus::Added)),
            (Path::new("a/b/c1.txt"), Some(GitFileStatus::Added)),
            (Path::new("a/b/c2.txt"), None),
            (Path::new("a/d"), Some(GitFileStatus::Modified)),
            (Path::new("a/d/e1.txt"), None),
            (Path::new("a/d/e2.txt"), Some(GitFileStatus::Modified)),
            (Path::new("f"), None),
            (Path::new("f/no-status.txt"), None),
            (Path::new("g"), Some(GitFileStatus::Conflict)),
        ],
    );

    check_propagated_statuses(
        &snapshot,
        &[
            (Path::new("a/b/c1.txt"), Some(GitFileStatus::Added)),
            (Path::new("a/b/c2.txt"), None),
            (Path::new("a/d/e1.txt"), None),
            (Path::new("a/d/e2.txt"), Some(GitFileStatus::Modified)),
            (Path::new("f/no-status.txt"), None),
        ],
    );

    #[track_caller]
    fn check_propagated_statuses(
        snapshot: &Snapshot,
        expected_statuses: &[(&Path, Option<GitFileStatus>)],
    ) {
        let mut entries = expected_statuses
            .iter()
            .map(|(path, _)| snapshot.entry_for_path(path).unwrap().clone())
            .collect::<Vec<_>>();
        snapshot.propagate_git_statuses(&mut entries);
        assert_eq!(
            entries
                .iter()
                .map(|e| (e.path.as_ref(), e.git_status))
                .collect::<Vec<_>>(),
            expected_statuses
        );
    }
}

#[track_caller]
fn git_init(path: &Path) -> git2::Repository {
    git2::Repository::init(path).expect("Failed to initialize git repository")
}

#[track_caller]
fn git_add<P: AsRef<Path>>(path: P, repo: &git2::Repository) {
    let path = path.as_ref();
    let mut index = repo.index().expect("Failed to get index");
    index.add_path(path).expect("Failed to add a.txt");
    index.write().expect("Failed to write index");
}

#[track_caller]
fn git_remove_index(path: &Path, repo: &git2::Repository) {
    let mut index = repo.index().expect("Failed to get index");
    index.remove_path(path).expect("Failed to add a.txt");
    index.write().expect("Failed to write index");
}

#[track_caller]
fn git_commit(msg: &'static str, repo: &git2::Repository) {
    use git2::Signature;

    let signature = Signature::now("test", "test@zed.dev").unwrap();
    let oid = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(oid).unwrap();
    if let Some(head) = repo.head().ok() {
        let parent_obj = head.peel(git2::ObjectType::Commit).unwrap();

        let parent_commit = parent_obj.as_commit().unwrap();

        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            msg,
            &tree,
            &[parent_commit],
        )
        .expect("Failed to commit with parent");
    } else {
        repo.commit(Some("HEAD"), &signature, &signature, msg, &tree, &[])
            .expect("Failed to commit");
    }
}

#[track_caller]
fn git_stash(repo: &mut git2::Repository) {
    use git2::Signature;

    let signature = Signature::now("test", "test@zed.dev").unwrap();
    repo.stash_save(&signature, "N/A", None)
        .expect("Failed to stash");
}

#[track_caller]
fn git_reset(offset: usize, repo: &git2::Repository) {
    let head = repo.head().expect("Couldn't get repo head");
    let object = head.peel(git2::ObjectType::Commit).unwrap();
    let commit = object.as_commit().unwrap();
    let new_head = commit
        .parents()
        .inspect(|parnet| {
            parnet.message();
        })
        .skip(offset)
        .next()
        .expect("Not enough history");
    repo.reset(&new_head.as_object(), git2::ResetType::Soft, None)
        .expect("Could not reset");
}

#[allow(dead_code)]
#[track_caller]
fn git_status(repo: &git2::Repository) -> collections::HashMap<String, git2::Status> {
    repo.statuses(None)
        .unwrap()
        .iter()
        .map(|status| (status.path().unwrap().to_string(), status.status()))
        .collect()
}