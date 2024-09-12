// Copyright 2022 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::path::Path;
use std::process::Command;

use assert_cmd::assert::OutputAssertExt;
use git2::Oid;

use crate::common::TestEnvironment;

#[test]
fn test_git_colocated() {
    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&workspace_root).unwrap();

    // Create an initial commit in Git
    std::fs::write(workspace_root.join("file"), "contents").unwrap();
    git_repo
        .index()
        .unwrap()
        .add_path(Path::new("file"))
        .unwrap();
    let tree1_oid = git_repo.index().unwrap().write_tree().unwrap();
    let tree1 = git_repo.find_tree(tree1_oid).unwrap();
    let signature = git2::Signature::new(
        "Someone",
        "someone@example.com",
        &git2::Time::new(1234567890, 60),
    )
    .unwrap();
    git_repo
        .commit(
            Some("refs/heads/master"),
            &signature,
            &signature,
            "initial",
            &tree1,
            &[],
        )
        .unwrap();
    insta::assert_snapshot!(
        git_repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
        @"e61b6729ff4292870702f2f72b2a60165679ef37"
    );

    // Import the repo
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  3e9369cd54227eb88455e1834dbc08aad6a16ac4
    ○  e61b6729ff4292870702f2f72b2a60165679ef37 master HEAD@git initial
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(
        git_repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
        @"e61b6729ff4292870702f2f72b2a60165679ef37"
    );

    // Modify the working copy. The working-copy commit should changed, but the Git
    // HEAD commit should not
    std::fs::write(workspace_root.join("file"), "modified").unwrap();
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  4f546c80f30abc0803fb83e5032a4d49fede4d68
    ○  e61b6729ff4292870702f2f72b2a60165679ef37 master HEAD@git initial
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(
        git_repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
        @"e61b6729ff4292870702f2f72b2a60165679ef37"
    );

    // Create a new change from jj and check that it's reflected in Git
    test_env.jj_cmd_ok(&workspace_root, &["new"]);
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  0e2301a42b288b9568344e32cfdd8c76d1e56a83
    ○  4f546c80f30abc0803fb83e5032a4d49fede4d68 HEAD@git
    ○  e61b6729ff4292870702f2f72b2a60165679ef37 master initial
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(
        git_repo.head().unwrap().target().unwrap().to_string(),
        @"4f546c80f30abc0803fb83e5032a4d49fede4d68"
    );
}

#[test]
fn test_git_colocated_unborn_bookmark() {
    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&workspace_root).unwrap();

    let add_file_to_index = |name: &str, data: &str| {
        std::fs::write(workspace_root.join(name), data).unwrap();
        let mut index = git_repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
    };
    let checkout_index = || {
        let mut index = git_repo.index().unwrap();
        index.read(true).unwrap(); // discard in-memory cache
        git_repo.checkout_index(Some(&mut index), None).unwrap();
    };

    // Initially, HEAD isn't set.
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);
    assert!(git_repo.head().is_err());
    assert_eq!(
        git_repo.find_reference("HEAD").unwrap().symbolic_target(),
        Some("refs/heads/master")
    );
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  230dd059e1b059aefc0da06a2e5a7dbf22362f22
    ◆  0000000000000000000000000000000000000000
    "###);

    // Stage some change, and check out root. This shouldn't clobber the HEAD.
    add_file_to_index("file0", "");
    let (stdout, stderr) = test_env.jj_cmd_ok(&workspace_root, &["new", "root()"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r###"
    Working copy now at: kkmpptxz fcdbbd73 (empty) (no description set)
    Parent commit      : zzzzzzzz 00000000 (empty) (no description set)
    Added 0 files, modified 0 files, removed 1 files
    "###);
    assert!(git_repo.head().is_err());
    assert_eq!(
        git_repo.find_reference("HEAD").unwrap().symbolic_target(),
        Some("refs/heads/master")
    );
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  fcdbbd731496cae17161cd6be9b6cf1f759655a8
    │ ○  993600f1189571af5bbeb492cf657dc7d0fde48a
    ├─╯
    ◆  0000000000000000000000000000000000000000
    "###);
    // Staged change shouldn't persist.
    checkout_index();
    insta::assert_snapshot!(test_env.jj_cmd_success(&workspace_root, &["status"]), @r###"
    The working copy is clean
    Working copy : kkmpptxz fcdbbd73 (empty) (no description set)
    Parent commit: zzzzzzzz 00000000 (empty) (no description set)
    "###);

    // Stage some change, and create new HEAD. This shouldn't move the default
    // bookmark.
    add_file_to_index("file1", "");
    let (stdout, stderr) = test_env.jj_cmd_ok(&workspace_root, &["new"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r###"
    Working copy now at: royxmykx 0e146103 (empty) (no description set)
    Parent commit      : kkmpptxz e3e01407 (no description set)
    "###);
    assert!(git_repo.head().unwrap().symbolic_target().is_none());
    insta::assert_snapshot!(
        git_repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
        @"e3e01407bd3539722ae4ffff077700d97c60cb11"
    );
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  0e14610343ef50775f5c44db5aeef19aee45d9ad
    ○  e3e01407bd3539722ae4ffff077700d97c60cb11 HEAD@git
    │ ○  993600f1189571af5bbeb492cf657dc7d0fde48a
    ├─╯
    ◆  0000000000000000000000000000000000000000
    "###);
    // Staged change shouldn't persist.
    checkout_index();
    insta::assert_snapshot!(test_env.jj_cmd_success(&workspace_root, &["status"]), @r###"
    The working copy is clean
    Working copy : royxmykx 0e146103 (empty) (no description set)
    Parent commit: kkmpptxz e3e01407 (no description set)
    "###);

    // Assign the default bookmark. The bookmark is no longer "unborn".
    test_env.jj_cmd_ok(&workspace_root, &["bookmark", "create", "-r@-", "master"]);

    // Stage some change, and check out root again. This should unset the HEAD.
    // https://github.com/martinvonz/jj/issues/1495
    add_file_to_index("file2", "");
    let (stdout, stderr) = test_env.jj_cmd_ok(&workspace_root, &["new", "root()"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r###"
    Working copy now at: znkkpsqq 10dd328b (empty) (no description set)
    Parent commit      : zzzzzzzz 00000000 (empty) (no description set)
    Added 0 files, modified 0 files, removed 2 files
    "###);
    assert!(git_repo.head().is_err());
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  10dd328bb906e15890e55047740eab2812a3b2f7
    │ ○  ef75c0b0dcc9b080e00226908c21316acaa84dc6
    │ ○  e3e01407bd3539722ae4ffff077700d97c60cb11 master
    ├─╯
    │ ○  993600f1189571af5bbeb492cf657dc7d0fde48a
    ├─╯
    ◆  0000000000000000000000000000000000000000
    "###);
    // Staged change shouldn't persist.
    checkout_index();
    insta::assert_snapshot!(test_env.jj_cmd_success(&workspace_root, &["status"]), @r###"
    The working copy is clean
    Working copy : znkkpsqq 10dd328b (empty) (no description set)
    Parent commit: zzzzzzzz 00000000 (empty) (no description set)
    "###);

    // New snapshot and commit can be created after the HEAD got unset.
    std::fs::write(workspace_root.join("file3"), "").unwrap();
    let (stdout, stderr) = test_env.jj_cmd_ok(&workspace_root, &["new"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r###"
    Working copy now at: wqnwkozp 101e2723 (empty) (no description set)
    Parent commit      : znkkpsqq fc8af934 (no description set)
    "###);
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  101e272377a9daff75358f10dbd078df922fe68c
    ○  fc8af9345b0830dcb14716e04cd2af26e2d19f63 HEAD@git
    │ ○  ef75c0b0dcc9b080e00226908c21316acaa84dc6
    │ ○  e3e01407bd3539722ae4ffff077700d97c60cb11 master
    ├─╯
    │ ○  993600f1189571af5bbeb492cf657dc7d0fde48a
    ├─╯
    ◆  0000000000000000000000000000000000000000
    "###);
}

#[test]
fn test_git_colocated_export_bookmarks_on_snapshot() {
    // Checks that we export bookmarks that were changed only because the working
    // copy was snapshotted

    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&workspace_root).unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);

    // Create bookmark pointing to the initial commit
    std::fs::write(workspace_root.join("file"), "initial").unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["bookmark", "create", "foo"]);
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  b15ef4cdd277d2c63cce6d67c1916f53a36141f7 foo
    ◆  0000000000000000000000000000000000000000
    "###);

    // The bookmark gets updated when we modify the working copy, and it should get
    // exported to Git without requiring any other changes
    std::fs::write(workspace_root.join("file"), "modified").unwrap();
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  4d2c49a8f8e2f1ba61f48ba79e5f4a5faa6512cf foo
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(git_repo
        .find_reference("refs/heads/foo")
        .unwrap()
        .target()
        .unwrap()
        .to_string(), @"4d2c49a8f8e2f1ba61f48ba79e5f4a5faa6512cf");
}

#[test]
fn test_git_colocated_rebase_on_import() {
    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&workspace_root).unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);

    // Make some changes in jj and check that they're reflected in git
    std::fs::write(workspace_root.join("file"), "contents").unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["commit", "-m", "add a file"]);
    std::fs::write(workspace_root.join("file"), "modified").unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["bookmark", "create", "master"]);
    test_env.jj_cmd_ok(&workspace_root, &["commit", "-m", "modify a file"]);
    // TODO: We shouldn't need this command here to trigger an import of the
    // refs/heads/master we just exported
    test_env.jj_cmd_ok(&workspace_root, &["st"]);

    // Move `master` backwards, which should result in commit2 getting hidden,
    // and the working-copy commit rebased.
    let commit2_oid = git_repo
        .find_branch("master", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let commit2 = git_repo.find_commit(commit2_oid).unwrap();
    let commit1 = commit2.parents().next().unwrap();
    git_repo.branch("master", &commit1, true).unwrap();
    let (stdout, stderr) = get_log_output_with_stderr(&test_env, &workspace_root);
    insta::assert_snapshot!(stdout, @r###"
    @  15b1d70c5e33b5d2b18383292b85324d5153ffed
    ○  47fe984daf66f7bf3ebf31b9cb3513c995afb857 master HEAD@git add a file
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(stderr, @r###"
    Abandoned 1 commits that are no longer reachable.
    Rebased 1 descendant commits off of commits rewritten from git
    Working copy now at: zsuskuln 15b1d70c (empty) (no description set)
    Parent commit      : qpvuntsm 47fe984d master | add a file
    Added 0 files, modified 1 files, removed 0 files
    Done importing changes from the underlying Git repo.
    "###);
}

#[test]
fn test_git_colocated_bookmarks() {
    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&workspace_root).unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);
    test_env.jj_cmd_ok(&workspace_root, &["new", "-m", "foo"]);
    test_env.jj_cmd_ok(&workspace_root, &["new", "@-", "-m", "bar"]);
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  3560559274ab431feea00b7b7e0b9250ecce951f bar
    │ ○  1e6f0b403ed2ff9713b5d6b1dc601e4804250cda foo
    ├─╯
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 HEAD@git
    ◆  0000000000000000000000000000000000000000
    "###);

    // Create a bookmark in jj. It should be exported to Git even though it points
    // to the working- copy commit.
    test_env.jj_cmd_ok(&workspace_root, &["bookmark", "create", "master"]);
    insta::assert_snapshot!(
        git_repo.find_reference("refs/heads/master").unwrap().target().unwrap().to_string(),
        @"3560559274ab431feea00b7b7e0b9250ecce951f"
    );
    insta::assert_snapshot!(
        git_repo.head().unwrap().target().unwrap().to_string(),
        @"230dd059e1b059aefc0da06a2e5a7dbf22362f22"
    );

    // Update the bookmark in Git
    let target_id = test_env.jj_cmd_success(
        &workspace_root,
        &["log", "--no-graph", "-T=commit_id", "-r=description(foo)"],
    );
    git_repo
        .reference(
            "refs/heads/master",
            Oid::from_str(&target_id).unwrap(),
            true,
            "test",
        )
        .unwrap();
    let (stdout, stderr) = get_log_output_with_stderr(&test_env, &workspace_root);
    insta::assert_snapshot!(stdout, @r###"
    @  096dc80da67094fbaa6683e2a205dddffa31f9a8
    │ ○  1e6f0b403ed2ff9713b5d6b1dc601e4804250cda master foo
    ├─╯
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 HEAD@git
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(stderr, @r###"
    Abandoned 1 commits that are no longer reachable.
    Working copy now at: yqosqzyt 096dc80d (empty) (no description set)
    Parent commit      : qpvuntsm 230dd059 (empty) (no description set)
    Done importing changes from the underlying Git repo.
    "###);
}

#[test]
fn test_git_colocated_bookmark_forget() {
    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    let _git_repo = git2::Repository::init(&workspace_root).unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);
    test_env.jj_cmd_ok(&workspace_root, &["new"]);
    test_env.jj_cmd_ok(&workspace_root, &["bookmark", "create", "foo"]);
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  65b6b74e08973b88d38404430f119c8c79465250 foo
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 HEAD@git
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(get_bookmark_output(&test_env, &workspace_root), @r###"
    foo: rlvkpnrz 65b6b74e (empty) (no description set)
      @git: rlvkpnrz 65b6b74e (empty) (no description set)
    "###);

    let (stdout, stderr) = test_env.jj_cmd_ok(&workspace_root, &["bookmark", "forget", "foo"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r###"
    Forgot 1 bookmarks.
    "###);
    // A forgotten bookmark is deleted in the git repo. For a detailed demo
    // explaining this, see `test_bookmark_forget_export` in
    // `test_bookmark_command.rs`.
    insta::assert_snapshot!(get_bookmark_output(&test_env, &workspace_root), @"");
}

#[test]
fn test_git_colocated_bookmark_at_root() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "--colocate", "repo"]);
    let repo_path = test_env.env_root().join("repo");

    let (_stdout, stderr) =
        test_env.jj_cmd_ok(&repo_path, &["bookmark", "create", "foo", "-r=root()"]);
    insta::assert_snapshot!(stderr, @r###"
    Created 1 bookmarks pointing to zzzzzzzz 00000000 foo | (empty) (no description set)
    Warning: Failed to export some bookmarks:
      foo: Ref cannot point to the root commit in Git
    "###);

    let (_stdout, stderr) = test_env.jj_cmd_ok(&repo_path, &["bookmark", "move", "foo"]);
    insta::assert_snapshot!(stderr, @r###"
    Moved 1 bookmarks to qpvuntsm 230dd059 foo | (empty) (no description set)
    "###);

    let (_stdout, stderr) = test_env.jj_cmd_ok(
        &repo_path,
        &[
            "bookmark",
            "move",
            "foo",
            "--allow-backwards",
            "--to=root()",
        ],
    );
    insta::assert_snapshot!(stderr, @r###"
    Moved 1 bookmarks to zzzzzzzz 00000000 foo* | (empty) (no description set)
    Warning: Failed to export some bookmarks:
      foo: Ref cannot point to the root commit in Git
    "###);
}

#[test]
fn test_git_colocated_conflicting_git_refs() {
    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    git2::Repository::init(&workspace_root).unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);
    test_env.jj_cmd_ok(&workspace_root, &["bookmark", "create", "main"]);
    let (stdout, stderr) = test_env.jj_cmd_ok(&workspace_root, &["bookmark", "create", "main/sub"]);
    insta::assert_snapshot!(stdout, @"");
    insta::with_settings!({filters => vec![("Failed to set: .*", "Failed to set: ...")]}, {
        insta::assert_snapshot!(stderr, @r###"
        Created 1 bookmarks pointing to qpvuntsm 230dd059 main main/sub | (empty) (no description set)
        Warning: Failed to export some bookmarks:
          main/sub: Failed to set: ...
        Hint: Git doesn't allow a branch name that looks like a parent directory of
        another (e.g. `foo` and `foo/bar`). Try to rename the bookmarks that failed to
        export or their "parent" bookmarks.
        "###);
    });
}

#[test]
fn test_git_colocated_checkout_non_empty_working_copy() {
    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&workspace_root).unwrap();
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);

    // Create an initial commit in Git
    // We use this to set HEAD to master
    std::fs::write(workspace_root.join("file"), "contents").unwrap();
    git_repo
        .index()
        .unwrap()
        .add_path(Path::new("file"))
        .unwrap();
    let tree1_oid = git_repo.index().unwrap().write_tree().unwrap();
    let tree1 = git_repo.find_tree(tree1_oid).unwrap();
    let signature = git2::Signature::new(
        "Someone",
        "someone@example.com",
        &git2::Time::new(1234567890, 60),
    )
    .unwrap();
    git_repo
        .commit(
            Some("refs/heads/master"),
            &signature,
            &signature,
            "initial",
            &tree1,
            &[],
        )
        .unwrap();
    insta::assert_snapshot!(
        git_repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
        @"e61b6729ff4292870702f2f72b2a60165679ef37"
    );

    std::fs::write(workspace_root.join("two"), "y").unwrap();

    test_env.jj_cmd_ok(&workspace_root, &["describe", "-m", "two"]);
    test_env.jj_cmd_ok(&workspace_root, &["new", "@-"]);
    let (_, stderr) = test_env.jj_cmd_ok(&workspace_root, &["describe", "-m", "new"]);
    insta::assert_snapshot!(stderr, @r###"
    Working copy now at: kkmpptxz 149cc31c (empty) new
    Parent commit      : lnksqltp e61b6729 master | initial
    "###);

    let git_head = git_repo.find_reference("HEAD").unwrap();
    let git_head_target = git_head.symbolic_target().unwrap();

    assert_eq!(git_head_target, "refs/heads/master");

    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  149cc31cb08a1589e6c5ee2cb2061559dc758ecb new
    │ ○  4ec6f6506bd1903410f15b80058a7f0d8f62deea two
    ├─╯
    ○  e61b6729ff4292870702f2f72b2a60165679ef37 master HEAD@git initial
    ◆  0000000000000000000000000000000000000000
    "###);
}

#[test]
fn test_git_colocated_fetch_deleted_or_moved_bookmark() {
    let test_env = TestEnvironment::default();
    test_env.add_config("git.auto-local-branch = true");
    let origin_path = test_env.env_root().join("origin");
    git2::Repository::init(&origin_path).unwrap();
    test_env.jj_cmd_ok(&origin_path, &["git", "init", "--git-repo=."]);
    test_env.jj_cmd_ok(&origin_path, &["describe", "-m=A"]);
    test_env.jj_cmd_ok(&origin_path, &["bookmark", "create", "A"]);
    test_env.jj_cmd_ok(&origin_path, &["new", "-m=B_to_delete"]);
    test_env.jj_cmd_ok(&origin_path, &["bookmark", "create", "B_to_delete"]);
    test_env.jj_cmd_ok(&origin_path, &["new", "-m=original C", "@-"]);
    test_env.jj_cmd_ok(&origin_path, &["bookmark", "create", "C_to_move"]);

    let clone_path = test_env.env_root().join("clone");
    git2::Repository::clone(origin_path.to_str().unwrap(), &clone_path).unwrap();
    test_env.jj_cmd_ok(&clone_path, &["git", "init", "--git-repo=."]);
    test_env.jj_cmd_ok(&clone_path, &["new", "A"]);
    insta::assert_snapshot!(get_log_output(&test_env, &clone_path), @r###"
    @  9c2de797c3c299a40173c5af724329012b77cbdd
    │ ○  4a191a9013d3f3398ccf5e172792a61439dbcf3a C_to_move original C
    ├─╯
    │ ○  c49ec4fb50844d0e693f1609da970b11878772ee B_to_delete B_to_delete
    ├─╯
    ◆  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 A HEAD@git A
    ◆  0000000000000000000000000000000000000000
    "###);

    test_env.jj_cmd_ok(&origin_path, &["bookmark", "delete", "B_to_delete"]);
    // Move bookmark C sideways
    test_env.jj_cmd_ok(&origin_path, &["describe", "C_to_move", "-m", "moved C"]);
    let (stdout, stderr) = test_env.jj_cmd_ok(&clone_path, &["git", "fetch"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r###"
    bookmark: B_to_delete@origin [deleted] untracked
    bookmark: C_to_move@origin   [updated] tracked
    Abandoned 2 commits that are no longer reachable.
    "###);
    // "original C" and "B_to_delete" are abandoned, as the corresponding bookmarks
    // were deleted or moved on the remote (#864)
    insta::assert_snapshot!(get_log_output(&test_env, &clone_path), @r###"
    ○  4f3d13296f978cbc351c46a43b4619c91b888475 C_to_move moved C
    │ @  9c2de797c3c299a40173c5af724329012b77cbdd
    ├─╯
    ◆  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 A HEAD@git A
    ◆  0000000000000000000000000000000000000000
    "###);
}

#[test]
fn test_git_colocated_rebase_dirty_working_copy() {
    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&repo_path).unwrap();
    test_env.jj_cmd_ok(&repo_path, &["git", "init", "--git-repo=."]);

    std::fs::write(repo_path.join("file"), "base").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["new"]);
    std::fs::write(repo_path.join("file"), "old").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["bookmark", "create", "feature"]);

    // Make the working-copy dirty, delete the checked out bookmark.
    std::fs::write(repo_path.join("file"), "new").unwrap();
    git_repo
        .find_reference("refs/heads/feature")
        .unwrap()
        .delete()
        .unwrap();

    // Because the working copy is dirty, the new working-copy commit will be
    // diverged. Therefore, the feature bookmark has change-delete conflict.
    let (stdout, stderr) = test_env.jj_cmd_ok(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout, @r###"
    Working copy changes:
    M file
    Working copy : rlvkpnrz 6bad94b1 feature?? | (no description set)
    Parent commit: qpvuntsm 3230d522 (no description set)
    These bookmarks have conflicts:
      feature
      Use `jj bookmark list` to see details. Use `jj bookmark set <name> -r <rev>` to resolve.
    "###);
    insta::assert_snapshot!(stderr, @r###"
    Warning: Failed to export some bookmarks:
      feature: Modified ref had been deleted in Git
    Done importing changes from the underlying Git repo.
    "###);
    insta::assert_snapshot!(get_log_output(&test_env, &repo_path), @r###"
    @  6bad94b10401f5fafc8a91064661224650d10d1b feature??
    ○  3230d52258f6de7e9afbd10da8d64503cc7cdca5 HEAD@git
    ◆  0000000000000000000000000000000000000000
    "###);

    // The working-copy content shouldn't be lost.
    insta::assert_snapshot!(
        std::fs::read_to_string(repo_path.join("file")).unwrap(), @"new");
}

#[test]
fn test_git_colocated_external_checkout() {
    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&repo_path).unwrap();
    let git_check_out_ref = |name| {
        git_repo
            .set_head_detached(git_repo.find_reference(name).unwrap().target().unwrap())
            .unwrap();
    };

    test_env.jj_cmd_ok(&repo_path, &["git", "init", "--git-repo=."]);
    test_env.jj_cmd_ok(&repo_path, &["ci", "-m=A"]);
    test_env.jj_cmd_ok(&repo_path, &["bookmark", "create", "-r@-", "master"]);
    test_env.jj_cmd_ok(&repo_path, &["new", "-m=B", "root()"]);
    test_env.jj_cmd_ok(&repo_path, &["new"]);

    // Checked out anonymous bookmark
    insta::assert_snapshot!(get_log_output(&test_env, &repo_path), @r###"
    @  f8a23336e41840ed1757ef323402a770427dc89a
    ○  eccedddfa5152d99fc8ddd1081b375387a8a382a HEAD@git B
    │ ○  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 master A
    ├─╯
    ◆  0000000000000000000000000000000000000000
    "###);

    // Check out another bookmark by external command
    git_check_out_ref("refs/heads/master");

    // The old working-copy commit gets abandoned, but the whole bookmark should not
    // be abandoned. (#1042)
    let (stdout, stderr) = get_log_output_with_stderr(&test_env, &repo_path);
    insta::assert_snapshot!(stdout, @r###"
    @  8bb9e8d42a37c2a4e8dcfad97fce0b8f49bc7afa
    ○  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 master HEAD@git A
    │ ○  eccedddfa5152d99fc8ddd1081b375387a8a382a B
    ├─╯
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(stderr, @r###"
    Reset the working copy parent to the new Git HEAD.
    "###);

    // Edit non-head commit
    test_env.jj_cmd_ok(&repo_path, &["new", "description(B)"]);
    test_env.jj_cmd_ok(&repo_path, &["new", "-m=C", "--no-edit"]);
    insta::assert_snapshot!(get_log_output(&test_env, &repo_path), @r###"
    ○  99a813753d6db988d8fc436b0d6b30a54d6b2707 C
    @  81e086b7f9b1dd7fde252e28bdcf4ba4abd86ce5
    ○  eccedddfa5152d99fc8ddd1081b375387a8a382a HEAD@git B
    │ ○  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 master A
    ├─╯
    ◆  0000000000000000000000000000000000000000
    "###);

    // Check out another bookmark by external command
    git_check_out_ref("refs/heads/master");

    // The old working-copy commit shouldn't be abandoned. (#3747)
    let (stdout, stderr) = get_log_output_with_stderr(&test_env, &repo_path);
    insta::assert_snapshot!(stdout, @r###"
    @  ca2a4e32f08688c6fb795c4c034a0a7e09c0d804
    ○  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 master HEAD@git A
    │ ○  99a813753d6db988d8fc436b0d6b30a54d6b2707 C
    │ ○  81e086b7f9b1dd7fde252e28bdcf4ba4abd86ce5
    │ ○  eccedddfa5152d99fc8ddd1081b375387a8a382a B
    ├─╯
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(stderr, @r###"
    Reset the working copy parent to the new Git HEAD.
    "###);
}

#[test]
fn test_git_colocated_squash_undo() {
    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    git2::Repository::init(&repo_path).unwrap();
    test_env.jj_cmd_ok(&repo_path, &["git", "init", "--git-repo=."]);
    test_env.jj_cmd_ok(&repo_path, &["ci", "-m=A"]);
    // Test the setup
    insta::assert_snapshot!(get_log_output_divergence(&test_env, &repo_path), @r###"
    @  rlvkpnrzqnoo 9670380ac379
    ○  qpvuntsmwlqt a7e4cec4256b A HEAD@git
    ◆  zzzzzzzzzzzz 000000000000
    "###);

    test_env.jj_cmd_ok(&repo_path, &["squash"]);
    insta::assert_snapshot!(get_log_output_divergence(&test_env, &repo_path), @r###"
    @  zsuskulnrvyr 6ee662324e5a
    ○  qpvuntsmwlqt 13ab6b96d82e A HEAD@git
    ◆  zzzzzzzzzzzz 000000000000
    "###);
    test_env.jj_cmd_ok(&repo_path, &["undo"]);
    // TODO: There should be no divergence here; 2f376ea1478c should be hidden
    // (#922)
    insta::assert_snapshot!(get_log_output_divergence(&test_env, &repo_path), @r###"
    @  rlvkpnrzqnoo 9670380ac379
    ○  qpvuntsmwlqt a7e4cec4256b A HEAD@git
    ◆  zzzzzzzzzzzz 000000000000
    "###);
}

#[test]
fn test_git_colocated_undo_head_move() {
    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&repo_path).unwrap();
    test_env.jj_cmd_ok(&repo_path, &["git", "init", "--git-repo=."]);

    // Create new HEAD
    test_env.jj_cmd_ok(&repo_path, &["new"]);
    insta::assert_snapshot!(
        git_repo.head().unwrap().target().unwrap().to_string(),
        @"230dd059e1b059aefc0da06a2e5a7dbf22362f22");
    insta::assert_snapshot!(get_log_output(&test_env, &repo_path), @r###"
    @  65b6b74e08973b88d38404430f119c8c79465250
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 HEAD@git
    ◆  0000000000000000000000000000000000000000
    "###);

    // HEAD should be unset
    test_env.jj_cmd_ok(&repo_path, &["undo"]);
    assert!(git_repo.head().is_err());
    insta::assert_snapshot!(get_log_output(&test_env, &repo_path), @r###"
    @  230dd059e1b059aefc0da06a2e5a7dbf22362f22
    ◆  0000000000000000000000000000000000000000
    "###);

    // Create commit on non-root commit
    test_env.jj_cmd_ok(&repo_path, &["new"]);
    test_env.jj_cmd_ok(&repo_path, &["new"]);
    insta::assert_snapshot!(get_log_output(&test_env, &repo_path), @r###"
    @  69b19f73cf584f162f078fb0d91c55ca39d10bc7
    ○  eb08b363bb5ef8ee549314260488980d7bbe8f63 HEAD@git
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(
        git_repo.head().unwrap().target().unwrap().to_string(),
        @"eb08b363bb5ef8ee549314260488980d7bbe8f63");

    // HEAD should be moved back
    let (stdout, stderr) = test_env.jj_cmd_ok(&repo_path, &["undo"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r#"
    Undid operation b50ec983d1c1 2001-02-03 04:05:13.000 +07:00 - 2001-02-03 04:05:13.000 +07:00 new empty commit
    Working copy now at: royxmykx eb08b363 (empty) (no description set)
    Parent commit      : qpvuntsm 230dd059 (empty) (no description set)
    "#);
    insta::assert_snapshot!(
        git_repo.head().unwrap().target().unwrap().to_string(),
        @"230dd059e1b059aefc0da06a2e5a7dbf22362f22");
    insta::assert_snapshot!(get_log_output(&test_env, &repo_path), @r###"
    @  eb08b363bb5ef8ee549314260488980d7bbe8f63
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 HEAD@git
    ◆  0000000000000000000000000000000000000000
    "###);
}

fn get_log_output_divergence(test_env: &TestEnvironment, repo_path: &Path) -> String {
    let template = r#"
    separate(" ",
      change_id.short(),
      commit_id.short(),
      description.first_line(),
      bookmarks,
      git_head,
      if(divergent, "!divergence!"),
    )
    "#;
    test_env.jj_cmd_success(repo_path, &["log", "-T", template])
}

fn get_log_output(test_env: &TestEnvironment, workspace_root: &Path) -> String {
    let template = r#"separate(" ", commit_id, bookmarks, git_head, working_copies, description)"#;
    test_env.jj_cmd_success(workspace_root, &["log", "-T", template, "-r=all()"])
}

fn get_log_output_with_stderr(
    test_env: &TestEnvironment,
    workspace_root: &Path,
) -> (String, String) {
    let template = r#"separate(" ", commit_id, bookmarks, git_head, working_copies, description)"#;
    test_env.jj_cmd_ok(workspace_root, &["log", "-T", template, "-r=all()"])
}

#[test]
fn test_git_colocated_unreachable_commits() {
    let test_env = TestEnvironment::default();
    let workspace_root = test_env.env_root().join("repo");
    let git_repo = git2::Repository::init(&workspace_root).unwrap();

    // Create an initial commit in Git
    let empty_tree_oid = git_repo.treebuilder(None).unwrap().write().unwrap();
    let tree1 = git_repo.find_tree(empty_tree_oid).unwrap();
    let signature = git2::Signature::new(
        "Someone",
        "someone@example.com",
        &git2::Time::new(1234567890, 60),
    )
    .unwrap();
    let oid1 = git_repo
        .commit(
            Some("refs/heads/master"),
            &signature,
            &signature,
            "initial",
            &tree1,
            &[],
        )
        .unwrap();
    insta::assert_snapshot!(
        git_repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
        @"2ee37513d2b5e549f7478c671a780053614bff19"
    );

    // Add a second commit in Git
    let tree2 = git_repo.find_tree(empty_tree_oid).unwrap();
    let signature = git2::Signature::new(
        "Someone",
        "someone@example.com",
        &git2::Time::new(1234567890, 62),
    )
    .unwrap();
    let oid2 = git_repo
        .commit(
            None,
            &signature,
            &signature,
            "next",
            &tree2,
            &[&git_repo.find_commit(oid1).unwrap()],
        )
        .unwrap();
    insta::assert_snapshot!(
        git_repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
        @"2ee37513d2b5e549f7478c671a780053614bff19"
    );

    // Import the repo while there is no path to the second commit
    test_env.jj_cmd_ok(&workspace_root, &["git", "init", "--git-repo", "."]);
    insta::assert_snapshot!(get_log_output(&test_env, &workspace_root), @r###"
    @  66ae47cee4f8c28ee8d7e4f5d9401b03c07e22f2
    ○  2ee37513d2b5e549f7478c671a780053614bff19 master HEAD@git initial
    ◆  0000000000000000000000000000000000000000
    "###);
    insta::assert_snapshot!(
        git_repo.head().unwrap().peel_to_commit().unwrap().id().to_string(),
        @"2ee37513d2b5e549f7478c671a780053614bff19"
    );

    // Check that trying to look up the second commit fails gracefully
    let stderr = test_env.jj_cmd_failure(&workspace_root, &["show", &oid2.to_string()]);
    insta::assert_snapshot!(stderr, @r###"
    Error: Revision "8e713ff77b54928dd4a82aaabeca44b1ae91722c" doesn't exist
    "###);
}

fn get_bookmark_output(test_env: &TestEnvironment, repo_path: &Path) -> String {
    // --quiet to suppress deleted bookmarks hint
    test_env.jj_cmd_success(repo_path, &["bookmark", "list", "--all-remotes", "--quiet"])
}

/// Substitute for `jj workspace add --colocate` or similar using git CLI,
/// please replace with the real thing when it lands.
fn stopgap_workspace_colocate(
    test_env: &TestEnvironment,
    repo_path: &Path,
    original_colocated: bool,
    dst: &str,
    initial_head: &str,
) {
    // Can't use gix/git2, as neither can repair the broken worktree we're about to
    // create.
    let repo_relative_path = if original_colocated {
        dst.to_owned()
    } else {
        format!("../../../../{dst}")
    };
    Command::new("git")
        .args(["worktree", "add", &repo_relative_path])
        .arg(initial_head)
        .current_dir(if original_colocated {
            repo_path.to_path_buf()
        } else {
            repo_path.join(".jj/repo/store/git")
        })
        .assert()
        .success()
        .stderr(format!(
            "Preparing worktree (detached HEAD {})\n",
            &initial_head[..7]
        ));
    let dst_path = repo_path.join(dst);
    let tmp_path = test_env.env_root().join("__tmp_worktree__");
    if tmp_path.exists() {
        std::fs::remove_dir_all(&tmp_path).unwrap();
    }
    std::fs::rename(&dst_path, &tmp_path).unwrap();
    test_env.jj_cmd_ok(repo_path, &["workspace", "add", dst]);
    std::fs::rename(tmp_path.join(".git"), dst_path.join(".git")).unwrap();
    std::fs::write(dst_path.join(".jj/.gitignore"), "*\n").unwrap();
    Command::new("git")
        .args(["worktree", "repair"])
        .current_dir(&dst_path)
        .assert()
        .success();
    Command::new("git")
        .arg("checkout")
        .arg(initial_head)
        .current_dir(&dst_path)
        .assert()
        .success();
}

#[test]
fn test_colocated_workspace_in_bare_repo() {
    // TODO: Remove when this stops requiring git (stopgap_workspace_colocate)
    if Command::new("git").arg("--version").status().is_err() {
        eprintln!("Skipping because git command might fail to run");
        return;
    }

    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    let second_path = test_env.env_root().join("second");
    //
    // git init without --colocate creates a bare repo
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    std::fs::write(repo_path.join("file"), "contents").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["commit", "-m", "initial commit"]);
    let (initial_commit, _) = test_env.jj_cmd_ok(
        &repo_path,
        &["log", "--no-graph", "-T", "commit_id", "-r", "@-"],
    );
    // TODO: replace with workspace add, when it can create worktrees
    stopgap_workspace_colocate(&test_env, &repo_path, false, "../second", &initial_commit);

    insta::assert_snapshot!(get_log_output(&test_env, &second_path), @r#"
    @  baf7f13355a30ddd3aa6476317fcbc9c65239b0c second@
    │ ○  45c9d8477181a2b9c077ff1b724694fe0969b301 default@
    ├─╯
    ○  046d74c8ab0a4730e58488508a5398b7a91e54a2 HEAD@git initial commit
    ◆  0000000000000000000000000000000000000000
    "#);

    test_env.jj_cmd_ok(
        &second_path,
        &["commit", "-m", "commit in second workspace"],
    );
    insta::assert_snapshot!(get_log_output(&test_env, &second_path), @r#"
    @  fca81879c29229d0097cb7d32fc8a661ee80c6e4 second@
    ○  220827d1ceb632ec7dd4cb2f5110b496977d14c2 HEAD@git commit in second workspace
    │ ○  45c9d8477181a2b9c077ff1b724694fe0969b301 default@
    ├─╯
    ○  046d74c8ab0a4730e58488508a5398b7a91e54a2 initial commit
    ◆  0000000000000000000000000000000000000000
    "#);

    // FIXME: There should still be no git HEAD in the default workspace, which
    // is not colocated. However, git_head() is a property of the view. And
    // currently, all colocated workspaces read and write from the same
    // entry of the common view.
    //
    // let stdout = test_env.jj_cmd_success(&repo_path, &["log", "--no-graph",
    // "-r", "git_head()"]); insta::assert_snapshot!(stdout, @r#""#);

    let stdout = test_env.jj_cmd_success(
        &second_path,
        &["op", "log", "-Tself.description().first_line()"],
    );
    insta::assert_snapshot!(stdout, @r#"
    @  commit baf7f13355a30ddd3aa6476317fcbc9c65239b0c
    ○  import git head
    ○  create initial working-copy commit in workspace second
    ○  add workspace 'second'
    ○  commit 4e8f9d2be039994f589b4e57ac5e9488703e604d
    ○  snapshot working copy
    ○  add workspace 'default'
    ○
    "#);
}

#[test]
fn test_colocated_workspace_moved_original_on_disk() {
    if Command::new("git").arg("--version").status().is_err() {
        eprintln!("Skipping because git command might fail to run");
        return;
    }

    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    let second_path = test_env.env_root().join("second");
    let new_repo_path = test_env.env_root().join("repo-moved");
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "--colocate", "repo"]);
    std::fs::write(repo_path.join("file"), "contents").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["commit", "-m", "initial commit"]);
    let (initial_commit, _) = test_env.jj_cmd_ok(
        &repo_path,
        &["log", "--no-graph", "-T", "commit_id", "-r", "@-"],
    );
    // TODO: replace with workspace add, when it can create worktrees
    stopgap_workspace_colocate(&test_env, &repo_path, true, "../second", &initial_commit);

    // Break our worktree by moving the original repo on disk
    std::fs::rename(&repo_path, &new_repo_path).unwrap();
    // imagine JJ were able to do this
    std::fs::write(
        second_path.join(".jj/repo"),
        new_repo_path
            .join(".jj/repo")
            .as_os_str()
            .as_encoded_bytes(),
    )
    .unwrap();

    let (_, stderr) = test_env.jj_cmd_ok(&second_path, &["status"]);
    insta::assert_snapshot!(stderr, @r#"
    Warning: Broken colocated git worktree.
    Hint: You may wish to try `git worktree repair` if you have moved the repo or worktree around.
    "#);

    Command::new("git")
        .args(["worktree", "repair"])
        .current_dir(&new_repo_path)
        .assert()
        .success();
    insta::assert_snapshot!(get_log_output(&test_env, &second_path), @r#"
    @  05530a3e0f9d581260343e273d66c381e76957df second@
    │ ○  45c9d8477181a2b9c077ff1b724694fe0969b301 default@
    ├─╯
    ○  046d74c8ab0a4730e58488508a5398b7a91e54a2 HEAD@git initial commit
    ◆  0000000000000000000000000000000000000000
    "#);
}

#[test]
fn test_colocated_workspace_wrong_gitdir() {
    // TODO: Remove when this stops requiring git (stopgap_workspace_colocate)
    if Command::new("git").arg("--version").status().is_err() {
        eprintln!("Skipping because git command might fail to run");
        return;
    }

    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    let second_path = test_env.env_root().join("second");
    let other_path = test_env.env_root().join("other");
    let other_second_path = test_env.env_root().join("other_second");
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "--colocate", "repo"]);
    std::fs::write(repo_path.join("file"), "contents").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["commit", "-m", "initial commit"]);
    let (initial_commit, _) = test_env.jj_cmd_ok(
        &repo_path,
        &["log", "--no-graph", "-T", "commit_id", "-r", "@-"],
    );
    // TODO: replace with workspace add, when it can create worktrees
    stopgap_workspace_colocate(&test_env, &repo_path, true, "../second", &initial_commit);

    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "--colocate", "other"]);
    std::fs::write(other_path.join("file"), "contents2").unwrap();
    test_env.jj_cmd_ok(&other_path, &["commit", "-m", "initial commit"]);
    let (ic_other, _) = test_env.jj_cmd_ok(
        &other_path,
        &["log", "--no-graph", "-T", "commit_id", "-r", "@-"],
    );
    // TODO: replace with workspace add, when it can create worktrees
    stopgap_workspace_colocate(&test_env, &other_path, true, "../other_second", &ic_other);

    // Break one of our worktrees
    std::fs::copy(other_second_path.join(".git"), second_path.join(".git")).unwrap();

    let (_, stderr) = test_env.jj_cmd_ok(&second_path, &["status"]);
    insta::assert_snapshot!(stderr, @r#"
    Warning: This workspace has a Git worktree that isn't managed by JJ; it points to a Git repo at $TEST_ENV/other/.git.
    "#);
}

#[test]
fn test_colocated_workspace_invalid_gitdir() {
    // TODO: Remove when this stops requiring git (stopgap_workspace_colocate)
    if Command::new("git").arg("--version").status().is_err() {
        eprintln!("Skipping because git command might fail to run");
        return;
    }

    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    let second_path = test_env.env_root().join("second");
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "--colocate", "repo"]);
    std::fs::write(repo_path.join("file"), "contents").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["commit", "-m", "initial commit"]);
    let (initial_commit, _) = test_env.jj_cmd_ok(
        &repo_path,
        &["log", "--no-graph", "-T", "commit_id", "-r", "@-"],
    );
    // TODO: replace with workspace add, when it can create worktrees
    stopgap_workspace_colocate(&test_env, &repo_path, true, "../second", &initial_commit);

    // Break one of our worktrees
    std::fs::write(second_path.join(".git"), "invalid").unwrap();

    let (_, stderr) = test_env.jj_cmd_ok(&second_path, &["status"]);
    insta::assert_snapshot!(stderr, @r#"
    Warning: Broken colocated git worktree.
    Hint: You may wish to try `git worktree repair` if you have moved the repo or worktree around.
    "#);
}

#[test]
fn test_colocated_workspace_independent_heads() {
    // TODO: Remove when this stops requiring git (stopgap_workspace_colocate)
    if Command::new("git").arg("--version").status().is_err() {
        eprintln!("Skipping because git command might fail to run");
        return;
    }

    let test_env = TestEnvironment::default();
    let repo_path = test_env.env_root().join("repo");
    let second_path = test_env.env_root().join("second");
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "--colocate", "repo"]);
    // create a commit so that git can have a HEAD
    std::fs::write(repo_path.join("file"), "contents").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["commit", "-m", "initial commit"]);
    let (initial_commit, _) = test_env.jj_cmd_ok(
        &repo_path,
        &["log", "--no-graph", "-T", "commit_id", "-r", "@-"],
    );
    // TODO: replace with workspace add, when it can create worktrees
    stopgap_workspace_colocate(&test_env, &repo_path, true, "../second", &initial_commit);

    {
        let first_git = git2::Repository::open(&repo_path).unwrap();
        assert!(first_git.head_detached().unwrap());
        let first_head = first_git.head().unwrap();

        let commit = first_head.peel_to_commit().unwrap().id();
        assert_eq!(commit.to_string(), initial_commit);

        let second_git = git2::Repository::open(&second_path).unwrap();
        assert!(second_git.head_detached().unwrap());
        let second_head = second_git.head().unwrap();

        let commit = second_head.peel_to_commit().unwrap().id();
        assert_eq!(commit.to_string(), initial_commit);
    }

    // now commit again in the second worktree, and make sure the original
    // repo's head does not move.
    //
    // This tests that we are writing HEAD to the corresponding worktree,
    // rather than unconditionally to the default workspace.
    std::fs::write(repo_path.join("file2"), "contents").unwrap();
    test_env.jj_cmd_ok(&second_path, &["commit", "-m", "followup commit"]);
    let (followup_commit, _) = test_env.jj_cmd_ok(
        &second_path,
        &["log", "--no-graph", "-T", "commit_id", "-r", "@-"],
    );

    {
        // git HEAD should not move in the default workspace
        let first_git = git2::Repository::open(&repo_path).unwrap();
        assert!(first_git.head_detached().unwrap());
        let first_head = first_git.head().unwrap();
        // still initial
        assert_eq!(
            first_head.peel_to_commit().unwrap().id().to_string(),
            initial_commit,
            "default workspace's git HEAD should not have moved from {initial_commit}"
        );

        let second_git = git2::Repository::open(&second_path).unwrap();
        assert!(second_git.head_detached().unwrap());
        let second_head = second_git.head().unwrap();
        assert_eq!(
            second_head.peel_to_commit().unwrap().id().to_string(),
            followup_commit,
            "second workspace's git HEAD should have advanced to {followup_commit}"
        );
    }

    // Finally, test imports. Test that a commit written to HEAD in one workspace
    // does not get imported by the other workspace.

    // Write in default, expect second not to import it
    let new_commit = test_independent_import(&test_env, &repo_path, &second_path, &followup_commit);
    // Write in second, expect default not to import it
    test_independent_import(&test_env, &second_path, &repo_path, &new_commit);

    fn test_independent_import(
        test_env: &TestEnvironment,
        commit_in: &Path,
        no_import_in_workspace: &Path,
        workspace_at: &str,
    ) -> String {
        // Commit in one workspace
        let mut repo = gix::open(commit_in).unwrap();
        {
            use gix::config::tree::*;
            let mut config = repo.config_snapshot_mut();
            let (name, email) = ("JJ test", "jj@example.com");
            config.set_value(&Author::NAME, name).unwrap();
            config.set_value(&Author::EMAIL, email).unwrap();
            config.set_value(&Committer::NAME, name).unwrap();
            config.set_value(&Committer::EMAIL, email).unwrap();
        }
        let tree = repo.head_tree_id().unwrap();
        let current = repo.head_commit().unwrap().id;
        let new_commit = repo
            .commit(
                "HEAD",
                format!("empty commit in {}", commit_in.display()),
                tree,
                [current],
            )
            .unwrap()
            .to_string();

        let (check_git_head, stderr) = test_env.jj_cmd_ok(
            no_import_in_workspace,
            &["log", "--no-graph", "-r", "git_head()", "-T", "commit_id"],
        );
        // Asserting stderr is empty => no import occurred
        assert_eq!(
            stderr,
            "",
            "Should not have imported HEAD in workspace {}",
            no_import_in_workspace.display()
        );
        // And the commit_id should be pointing to what it was before
        assert_eq!(
            check_git_head,
            workspace_at,
            "should still be at {workspace_at} in workspace {}",
            no_import_in_workspace.display()
        );

        // Now we import the new HEAD in the commit_in workspace, so it's up to date.
        let (check_git_head, stderr) = test_env.jj_cmd_ok(
            commit_in,
            &["log", "--no-graph", "-r", "git_head()", "-T", "commit_id"],
        );
        assert_eq!(
            stderr,
            "Reset the working copy parent to the new Git HEAD.\n",
            "should have imported HEAD in workspace {}",
            commit_in.display()
        );
        assert_eq!(
            check_git_head,
            new_commit,
            "should have advanced to {new_commit} in workspace {}",
            commit_in.display()
        );
        new_commit
    }
}
