use super::*;

fn proposed(result: CommandResult) -> CommandProposal {
    match result {
        CommandResult::Proposed(proposal) => proposal,
        other => panic!("expected proposal, got {other:?}"),
    }
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
        .create_game(SidePreference::White, false, &hash)
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
