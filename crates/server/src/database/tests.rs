use super::*;

fn proposed(result: CommandResult) -> CommandProposal {
    match result {
        CommandResult::Proposed(proposal) => proposal,
        other => panic!("expected proposal, got {other:?}"),
    }
}

fn undo_proposed(result: CommandResult) -> UndoProposal {
    match result {
        CommandResult::UndoProposed(proposal) => proposal,
        other => panic!("expected undo proposal, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in NOLAN_TEST_DATABASE_URL"]
async fn lab_setting_is_opaque_and_survives_join_reconnect_and_gameplay() {
    let pool = sqlx::PgPool::connect(&std::env::var("NOLAN_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let repo = Repository::new(pool.clone());
    let hash = "e".repeat(64);
    // Deliberately not engine data. The server only stores the envelope, even
    // for settings larger than the previous 16 KiB websocket frame limit.
    let setting = "s".repeat(128 * 1024);
    let ready = repo
        .create_game(
            SidePreference::White,
            true,
            true,
            false,
            &hash,
            RoomStart {
                time_war_separation: None,
                lab_setup: Some(setting.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(ready.lab_setup.as_ref(), Some(&setting));
    let white = ready.auth;
    let black = repo
        .join_game(&white.game_id, None, &hash, false)
        .await
        .unwrap();
    assert_eq!(black.lab_setup.as_ref(), Some(&setting));
    let p = proposed(
        repo.propose_command(&white, 0, "opaque Lab move")
            .await
            .unwrap(),
    );
    repo.vote_command(&white, &p.id, 0, true, None)
        .await
        .unwrap();
    repo.vote_command(&black.auth, &p.id, 0, true, None)
        .await
        .unwrap();
    let resumed = repo
        .join_game(&white.game_id, Some(&white.player_token), &hash, false)
        .await
        .unwrap();
    assert_eq!(resumed.lab_setup, Some(setting));
    assert_eq!(resumed.history, ["opaque Lab move"]);
    assert!(
        repo.create_game(
            SidePreference::White,
            true,
            true,
            false,
            &hash,
            RoomStart {
                time_war_separation: Some(9),
                lab_setup: Some("conflict".into())
            }
        )
        .await
        .is_err()
    );
    sqlx::query("DELETE FROM games WHERE game_id = $1")
        .bind(&white.game_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in NOLAN_TEST_DATABASE_URL"]
async fn undo_requires_opponent_consent_and_keeps_revisions_monotonic() {
    let pool = sqlx::PgPool::connect(&std::env::var("NOLAN_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let repo = Repository::new(pool.clone());
    let hash = "d".repeat(64);
    let white = repo
        .create_game(SidePreference::White, true, true, false, &hash, None.into())
        .await
        .unwrap()
        .auth;
    let black = repo
        .join_game(&white.game_id, None, &hash, false)
        .await
        .unwrap()
        .auth;
    // Opaque strings demonstrate that undo never runs chess code on the server.
    for (revision, command) in ["first", "second", "free continuation"]
        .into_iter()
        .enumerate()
    {
        let p = proposed(
            repo.propose_command(&white, revision as u64, command)
                .await
                .unwrap(),
        );
        repo.vote_command(&white, &p.id, p.revision, true, None)
            .await
            .unwrap();
        assert!(matches!(
            repo.vote_command(&black, &p.id, p.revision, true, None)
                .await
                .unwrap(),
            CommandResult::Accepted { .. }
        ));
    }
    assert!(repo.request_undo(&white, 3, 3).await.is_err());
    let request = undo_proposed(repo.request_undo(&white, 3, 1).await.unwrap());
    assert_eq!(
        request,
        undo_proposed(repo.request_undo(&white, 3, 1).await.unwrap())
    );
    let resumed = repo
        .join_game(&black.game_id, Some(&black.player_token), &hash, false)
        .await
        .unwrap();
    assert_eq!(resumed.pending_undo, Some(request.clone()));
    assert!(resumed.pending.is_none());
    assert_eq!(resumed.history, ["first", "second", "free continuation"]);
    assert!(matches!(
        repo.vote_undo(&white, &request.id, 3, true).await,
        Err(RepositoryError::Client {
            code: "invalid_undo_vote",
            ..
        })
    ));
    let mut forged = black.clone();
    forged.player_token = white.player_token.clone();
    assert!(matches!(
        repo.vote_undo(&forged, &request.id, 3, true).await,
        Err(RepositoryError::Client {
            code: "invalid_player_token",
            ..
        })
    ));
    // The automatic command-validation channel cannot approve an undo.
    assert!(matches!(
        repo.vote_command(&black, &request.id, 3, true, None)
            .await
            .unwrap(),
        CommandResult::Unchanged
    ));
    assert!(matches!(
        repo.propose_command(&black, 3, "blocked").await,
        Err(RepositoryError::Client {
            code: "proposal_pending",
            ..
        })
    ));
    assert!(matches!(
        repo.vote_undo(&black, &request.id, 3, false).await.unwrap(),
        CommandResult::UndoDeclined { .. }
    ));
    assert!(matches!(repo.resync(&white).await.unwrap(),
        CommandResult::Sync { revision: 3, history, pending_undo: None, .. } if history.len() == 3));

    let next = undo_proposed(repo.request_undo(&black, 3, 1).await.unwrap());
    assert_ne!(request.id, next.id);
    assert!(
        matches!(repo.vote_undo(&black, &request.id, 3, true).await.unwrap(),
        CommandResult::Sync { pending_undo: Some(p), .. } if p == next)
    );
    assert!(matches!(
        repo.vote_undo(&white, &next.id, 2, true).await.unwrap(),
        CommandResult::Sync { revision: 3, .. }
    ));
    let (a, b) = tokio::join!(
        repo.vote_undo(&white, &next.id, 3, true),
        repo.vote_undo(&white, &next.id, 3, true),
    );
    let results = [a.unwrap(), b.unwrap()];
    assert_eq!(results.iter().filter(|r| matches!(r, CommandResult::UndoAccepted { revision: 4, history, .. } if history == &["first"])).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, CommandResult::Sync { revision: 4, .. }))
            .count(),
        1
    );
    let resumed = repo
        .join_game(&white.game_id, Some(&white.player_token), &hash, false)
        .await
        .unwrap();
    assert_eq!(resumed.revision, 4);
    assert_eq!(resumed.history, ["first"]);
    assert!(resumed.pending_undo.is_none());
    assert!(matches!(
        repo.propose_command(&white, 3, "stale").await.unwrap(),
        CommandResult::Sync { revision: 4, .. }
    ));
    let new_move = proposed(
        repo.propose_command(&white, 4, "replacement")
            .await
            .unwrap(),
    );
    repo.vote_command(&white, &new_move.id, 4, true, None)
        .await
        .unwrap();
    assert!(matches!(
        repo.vote_command(&black, &new_move.id, 4, true, None)
            .await
            .unwrap(),
        CommandResult::Accepted { revision: 5, .. }
    ));
    assert!(
        matches!(repo.resync(&black).await.unwrap(), CommandResult::Sync { revision: 5, history, .. } if history == ["first", "replacement"])
    );
    let undo_again = undo_proposed(repo.request_undo(&white, 5, 0).await.unwrap());
    assert!(
        matches!(repo.vote_undo(&black, &undo_again.id, 5, true).await.unwrap(), CommandResult::UndoAccepted { revision: 6, history, .. } if history.is_empty())
    );
    assert!(repo.request_undo(&white, 6, 0).await.is_err());

    sqlx::query("DELETE FROM games WHERE game_id = $1")
        .bind(&white.game_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in NOLAN_TEST_DATABASE_URL"]
async fn two_client_consensus_persists_and_commits_exactly_once() {
    let url = std::env::var("NOLAN_TEST_DATABASE_URL").unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let repo = Repository::new(pool.clone());
    let hash = "a".repeat(64);
    let new_hash = "b".repeat(64);
    let white = repo
        .create_game(
            SidePreference::White,
            false,
            false,
            false,
            &hash,
            None.into(),
        )
        .await
        .unwrap()
        .auth;
    assert!(matches!(
        repo.join_game(&white.game_id, None, &new_hash, false).await,
        Err(RepositoryError::Client {
            code: "engine_revision_mismatch",
            ..
        })
    ));
    let black = repo
        .join_game(&white.game_id, None, &hash, false)
        .await
        .unwrap()
        .auth;
    assert_eq!(black.side, Side::Black);

    // Deliberately not chess: the server must neither parse nor run this command.
    let command = "opaque command understood only by clients";
    let proposal = proposed(repo.propose_command(&white, 0, command).await.unwrap());
    assert_eq!(
        proposal,
        proposed(repo.propose_command(&white, 0, command).await.unwrap())
    );
    assert!(matches!(
        repo.propose_command(&black, 0, "other").await,
        Err(RepositoryError::Client {
            code: "proposal_pending",
            ..
        })
    ));
    for _ in 0..2 {
        assert!(matches!(
            repo.vote_command(&white, &proposal.id, 0, true, None)
                .await
                .unwrap(),
            CommandResult::Unchanged
        ));
    }
    let reconnected = repo
        .join_game(&white.game_id, Some(&white.player_token), &hash, false)
        .await
        .unwrap();
    assert_eq!(reconnected.revision, 0);
    assert!(reconnected.history.is_empty());
    assert_eq!(reconnected.pending, Some(proposal.clone()));

    let mut impersonator = black.clone();
    impersonator.player_token = white.player_token.clone();
    assert!(matches!(
        repo.vote_command(&impersonator, &proposal.id, 0, true, None)
            .await,
        Err(RepositoryError::Client {
            code: "invalid_player_token",
            ..
        })
    ));

    // A duplicate vote racing the real vote must not append another journal entry.
    let (first, duplicate) = tokio::join!(
        repo.vote_command(&black, &proposal.id, 0, true, None),
        repo.vote_command(&black, &proposal.id, 0, true, None),
    );
    let results = [first.unwrap(), duplicate.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, CommandResult::Accepted { revision: 1, .. }))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, CommandResult::Unchanged))
            .count(),
        1
    );

    let rejected = proposed(repo.propose_command(&black, 1, "rejected").await.unwrap());
    repo.vote_command(&black, &rejected.id, 1, true, None)
        .await
        .unwrap();
    assert!(
        matches!(repo.vote_command(&white, &rejected.id, 1, false, Some("illegal")).await.unwrap(),
        CommandResult::Rejected { side: Side::White, reason, .. } if reason == "illegal")
    );
    assert!(matches!(
        repo.vote_command(&white, &rejected.id, 1, true, None)
            .await
            .unwrap(),
        CommandResult::Unchanged
    ));
    assert!(matches!(repo.resync(&white).await.unwrap(),
        CommandResult::Sync { revision: 1, history, pending: None, .. } if history == [command]));

    let pending = proposed(repo.propose_command(&white, 1, "next").await.unwrap());
    assert!(
        matches!(repo.propose_command(&black, 0, "stale").await.unwrap(),
        CommandResult::Sync { revision: 1, pending: Some(p), .. } if p == pending)
    );
    repo.vote_command(&white, &pending.id, 1, true, None)
        .await
        .unwrap();
    // Only an empty room can adopt a new build; previous votes cannot carry over.
    assert!(
        repo.join_game(&white.game_id, Some(&white.player_token), &new_hash, false)
            .await
            .is_err()
    );
    let upgraded = repo
        .join_game(&white.game_id, Some(&white.player_token), &new_hash, true)
        .await
        .unwrap();
    assert_eq!(upgraded.engine_revision, new_hash);
    assert_eq!(upgraded.history, [command]);
    assert!(upgraded.pending.is_none());
    assert!(matches!(
        repo.vote_command(&black, &pending.id, 1, true, None)
            .await
            .unwrap(),
        CommandResult::Unchanged
    ));

    let expired = proposed(repo.propose_command(&white, 1, "expires").await.unwrap());
    sqlx::query(
        "UPDATE command_proposals SET created_at = now() - interval '6 minutes' WHERE game_id = $1",
    )
    .bind(&white.game_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        repo.vote_command(&black, &expired.id, 1, true, None)
            .await
            .unwrap(),
        CommandResult::Unchanged
    ));
    let fresh = proposed(repo.propose_command(&white, 1, "expires").await.unwrap());
    assert_ne!(expired.id, fresh.id);
    assert!(
        repo.propose_command(&white, 1, "two\ncommands")
            .await
            .is_err()
    );
    assert!(
        repo.propose_command(&white, 1, &"x".repeat(MAX_COMMAND_LEN + 1))
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM games WHERE game_id = $1")
        .bind(&white.game_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database in NOLAN_TEST_DATABASE_URL"]
async fn two_client_consensus_keeps_independent_room_rules() {
    let pool = sqlx::PgPool::connect(&std::env::var("NOLAN_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let repo = Repository::new(pool.clone());
    let hash = "c".repeat(64);
    for chain in [false, true] {
        for global in [false, true] {
            let created = repo
                .create_game(
                    SidePreference::White,
                    chain,
                    global,
                    chain,
                    &hash,
                    Some(9).into(),
                )
                .await
                .unwrap();
            assert_eq!(
                (created.chain_info_activation, created.global_reaction_reset),
                (chain, global)
            );
            assert_eq!(created.wait_before_move, chain);
            let joined = repo
                .join_game(&created.auth.game_id, None, &hash, false)
                .await
                .unwrap();
            assert_eq!(joined.time_war_separation, Some(9));
            assert_eq!(joined.wait_before_move, chain);
            assert_eq!(
                (joined.chain_info_activation, joined.global_reaction_reset),
                (chain, global)
            );
            let resumed = repo
                .join_game(
                    &created.auth.game_id,
                    Some(&created.auth.player_token),
                    &hash,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(resumed.time_war_separation, Some(9));
            assert_eq!(resumed.wait_before_move, chain);
            assert_eq!(
                (resumed.chain_info_activation, resumed.global_reaction_reset),
                (chain, global)
            );
            // Do not reinterpret rooms created under the removed ruleset.
            sqlx::query("UPDATE games SET causal_gaps = TRUE WHERE game_id = $1")
                .bind(&created.auth.game_id)
                .execute(&pool)
                .await
                .unwrap();
            assert!(matches!(
                repo.join_game(
                    &created.auth.game_id,
                    Some(&created.auth.player_token),
                    &hash,
                    false
                )
                .await,
                Err(RepositoryError::Client {
                    code: "removed_room_rules",
                    ..
                })
            ));
            sqlx::query("DELETE FROM games WHERE game_id = $1")
                .bind(&created.auth.game_id)
                .execute(&pool)
                .await
                .unwrap();
        }
    }
    pool.close().await;
}
