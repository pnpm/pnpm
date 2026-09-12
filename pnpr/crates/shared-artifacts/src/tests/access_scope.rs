use super::{
    Arc, FailArtifactWrites, FailOnly, HostedStoreConfig, InMemory, ObjectStore, RegistryError,
    SharedArtifactStore, TempDir, publication, publication_tagged,
};

/// Matching a tag set is order-independent, so two orderings are the same
/// constraint and must not be two slots — otherwise a publisher reopens the
/// swap simply by listing the same tags the other way round.
#[tokio::test]
async fn tag_order_does_not_open_a_second_slot() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17", "pnpm:v1:linux-arm64-node22-glibc2.17"];
    assert!(store.publish("acme", publication_tagged("ci/first", &tags)).await.unwrap());

    let reversed = [tags[1], tags[0]];
    let error =
        store.publish("acme", publication_tagged("ci/second", &reversed)).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

/// A tagged artifact outranks a universal one for every consumer its tag fits,
/// so publishing one over a universal artifact would decide what those
/// consumers run without ever refilling a slot.
#[tokio::test]
async fn a_tagged_artifact_cannot_supersede_a_universal_one() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    assert!(store.publish("acme", publication("ci/first")).await.unwrap());

    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let error = store.publish("acme", publication_tagged("ci/second", &tags)).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

#[tokio::test]
async fn a_universal_artifact_cannot_supersede_a_tagged_one() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    assert!(store.publish("acme", publication_tagged("ci/first", &tags)).await.unwrap());

    let error = store.publish("acme", publication("ci/second")).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

/// A consumer meeting the higher floor meets the lower one as well, so both
/// artifacts would apply to it and ranking would pick between them.
#[tokio::test]
async fn a_second_floor_for_one_platform_cannot_open_a_second_slot() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let floor = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    assert!(store.publish("acme", publication_tagged("ci/first", &floor)).await.unwrap());

    let raised = ["pnpm:v1:linux-x64-node22-glibc2.31"];
    let error = store.publish("acme", publication_tagged("ci/second", &raised)).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

/// The marker, not the variant path, is what two publications contend on, and
/// nothing of the refused one is written: it is turned away before it stores
/// anything, and what it lost to is left as it was.
#[tokio::test]
async fn a_scope_another_artifact_holds_refuses_publication() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let held = publication_tagged("ci/held", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let holder = held.envelope.digest().unwrap();
    let (payload, _) = held.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&held.key, &payload.subject);
    assert!(store.publish("acme", held).await.unwrap());

    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.31"]);
    let slot = {
        let (payload, _) = ours.envelope.decode_payload().unwrap();
        super::super::compatibility_slot(&payload.compatibility)
    };
    let error = store.publish("acme", ours).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
    assert!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/{slot}.json"), 4096)
            .await
            .unwrap()
            .is_none(),
        "nothing is written before the claim succeeds",
    );
    assert_eq!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-x64-node22"), 128)
            .await
            .unwrap()
            .as_deref(),
        Some(holder.as_bytes()),
        "and the marker it lost on still names the artifact holding it",
    );
}

/// A scope this publication reserved and did not keep has to go back, or the
/// artifact that should hold it could never be published.
#[tokio::test]
async fn a_publication_that_fails_gives_back_the_scopes_it_claimed() {
    let failing: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner: InMemory::new(),
        commit_before_error: false,
        fail_deletes: false,
        fail_next_quota_write: None,
        claim_slot_first: None,
        fail_slot_read_after_first: None,
        publish_overlapping_after_create: None,
        fail_reads_of: None,
        fail_scope_writes: false,
        fail_only: None,
        usage_writes: None,
    });
    let store = SharedArtifactStore::new(
        &HostedStoreConfig::ObjectStore { store: failing, prefix: String::new() },
        TempDir::new().unwrap().path(),
    )
    .unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];

    store.publish("acme", publication_tagged("ci/first", &tags)).await.unwrap_err();

    let (payload, _) = publication_tagged("ci/first", &tags).envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry =
        super::super::entry_digest(&publication_tagged("ci/first", &tags).key, &payload.subject);
    assert!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-x64-node22"), 128)
            .await
            .unwrap()
            .is_none(),
        "the scope is free for the artifact that does get stored",
    );
}

/// A marker that loses the create and is gone by the time it is read belongs to
/// nobody. Reading that as this artifact's own would store it reserving
/// nothing, and an overlapping artifact could follow it in.
#[tokio::test]
async fn a_scope_that_vanishes_after_the_create_is_not_taken_as_ours() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&ours.key, &payload.subject);

    assert_eq!(
        store.scope_marker(&owner, &entry, "linux-x64-node22", "any-digest").await.unwrap(),
        super::super::ScopeMarker::Gone,
        "an absent marker is nobody's, not this artifact's",
    );
}

/// A publication that claims scopes and then fails leaves them claimed: it
/// cannot tell its own abandoned marker from one a publication of the same
/// envelope is using right now. Reclamation runs when none is in flight, so it
/// can, and the scope goes back there.
#[tokio::test]
async fn a_scope_a_failed_publication_left_behind_is_reclaimed() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let ours = publication_tagged("ci/ours", &tags);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&ours.key, &payload.subject);
    let marker = format!("{owner}/entries/{entry}/scopes/linux-x64-node22");
    // What a publication that claimed the scope and then failed leaves.
    store.create_object(&marker, b"an artifact nobody stored".to_vec()).await.unwrap();

    store.reclaim_unreferenced_blobs().await.unwrap();

    assert!(
        store.read_object_bounded(&marker, 128).await.unwrap().is_none(),
        "a scope no stored artifact holds goes back",
    );
    assert!(
        store.publish("acme", publication_tagged("ci/later", &tags)).await.unwrap(),
        "and the artifact that should hold it can be published",
    );
}

/// Reclamation drops what no artifact holds, not what a stored one does.
#[tokio::test]
async fn reclamation_keeps_the_scopes_a_stored_artifact_reaches() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    assert!(store.publish("acme", publication_tagged("ci/stored", &tags)).await.unwrap());

    store.reclaim_unreferenced_blobs().await.unwrap();

    let raised = ["pnpm:v1:linux-x64-node22-glibc2.31"];
    let error = store.publish("acme", publication_tagged("ci/raised", &raised)).await.unwrap_err();
    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "the stored artifact still reaches its machines, got {error:?}",
    );
}

/// Once a crowded entry has been given its markers, each artifact holds the
/// scope it reaches, and looking only at its own would report both as already
/// published. Reaching the same machines from the other side of the vocabulary
/// is what a retry into such an entry has to be refused for.
#[tokio::test]
async fn a_retry_into_a_crowded_entry_is_refused_once_its_scopes_are_known() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let universal = publication("ci/universal");
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let tagged = publication_tagged("ci/tagged", &tags);
    let (payload, _) = universal.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&universal.key, &payload.subject);
    // Each under the name its own constraints give it, which is where a store
    // written when only identical constraints conflicted put them.
    for request in [&universal, &tagged] {
        let (payload, _) = request.envelope.decode_payload().unwrap();
        let slot = super::super::compatibility_slot(&payload.compatibility);
        store
            .create_object(
                &format!("{owner}/entries/{entry}/{slot}.json"),
                serde_json::to_vec(&request.envelope).unwrap(),
            )
            .await
            .unwrap();
    }
    // Gives the entry the markers its artifacts reach, and is itself refused.
    store.publish("acme", publication("ci/third")).await.unwrap_err();

    for republished in [publication("ci/universal"), publication_tagged("ci/tagged", &tags)] {
        let error = store.publish("acme", republished).await.unwrap_err();
        assert!(
            matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
            "a retry into a crowded entry is refused, got {error:?}",
        );
    }
}

/// Writing off a publication that is merely slow lets reclamation give back
/// scopes it is still holding. Its artifact is stored all the same, so it takes
/// them back rather than being left reaching machines nothing says it reaches.
#[tokio::test]
async fn a_publication_written_off_while_running_takes_its_scopes_back() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let ours = publication_tagged("ci/ours", &tags);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&ours.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let marker = format!("{owner}/entries/{entry}/scopes/linux-x64-node22");
    // The artifact is stored and its scope has been given back, which is where a
    // publication written off mid-flight finds things when it comes to finish.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/{slot}.json"),
            serde_json::to_vec(&ours.envelope).unwrap(),
        )
        .await
        .unwrap();

    store
        .recover_after_expiry(
            &owner,
            &entry,
            &format!("{owner}/entries/{entry}/{slot}.json"),
            &payload,
            &ours.envelope.digest().unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        store.read_object_bounded(&marker, 128).await.unwrap().as_deref(),
        Some(ours.envelope.digest().unwrap().as_bytes()),
        "the stored artifact reaches its machines again",
    );
    let raised = ["pnpm:v1:linux-x64-node22-glibc2.31"];
    let error = store.publish("acme", publication_tagged("ci/raised", &raised)).await.unwrap_err();
    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "and one reaching the same machines is refused again, got {error:?}",
    );
}

/// A scope can have gone to an artifact published while this one was written
/// off, and that artifact holds it. This one is then reaching a machine nothing
/// says it reaches, so it takes its own artifact back out rather than leaving
/// two that reach it.
#[tokio::test]
async fn a_publication_whose_scope_went_elsewhere_takes_its_artifact_back_out() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&ours.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let variant = format!("{owner}/entries/{entry}/{slot}.json");
    store.create_object(&variant, serde_json::to_vec(&ours.envelope).unwrap()).await.unwrap();
    // Published while this one was written off, and holding the scope now.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/scopes/linux-x64-node22"),
            b"an artifact published meanwhile".to_vec(),
        )
        .await
        .unwrap();

    let error = store
        .recover_after_expiry(&owner, &entry, &variant, &payload, &ours.envelope.digest().unwrap())
        .await
        .unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "the publication is told it lost, got {error:?}",
    );
    assert!(
        store.read_object_bounded(&variant, 4096).await.unwrap().is_none(),
        "and its artifact does not stay beside the one that holds the scope",
    );
}

/// The artifact goes out before the scopes it retook, so that a store error
/// between the two never leaves it resolvable while holding nothing.
#[tokio::test]
async fn a_recovery_that_cannot_remove_its_artifact_keeps_the_scopes_it_retook() {
    let tags = ["pnpm:v1:linux-arm64-node22-glibc2.17", "pnpm:v1:linux-x64-node22-glibc2.17"];
    let ours = publication_tagged("ci/ours", &tags);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&ours.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let variant = format!("{owner}/entries/{entry}/{slot}.json");
    let backend: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner: InMemory::new(),
        commit_before_error: false,
        fail_deletes: false,
        fail_next_quota_write: None,
        claim_slot_first: None,
        fail_slot_read_after_first: None,
        publish_overlapping_after_create: None,
        fail_reads_of: None,
        fail_scope_writes: false,
        fail_only: Some(FailOnly::DeleteOf(format!(".pnpr-artifacts/v0/{variant}"))),
        usage_writes: None,
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    store.create_object(&variant, serde_json::to_vec(&ours.envelope).unwrap()).await.unwrap();
    // Taken while this publication was written off, so the recovery loses — but
    // only after retaking the scope that sorts before it.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/scopes/linux-x64-node22"),
            b"an artifact published meanwhile".to_vec(),
        )
        .await
        .unwrap();

    let error = store
        .recover_after_expiry(&owner, &entry, &variant, &payload, &ours.envelope.digest().unwrap())
        .await
        .unwrap_err();

    assert!(matches!(error, RegistryError::ObjectStore(_)), "{error:?}");
    assert_eq!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-arm64-node22"), 128)
            .await
            .unwrap()
            .as_deref(),
        Some(ours.envelope.digest().unwrap().as_bytes()),
        "the artifact that is still there still holds the scope it retook",
    );
}

/// The other form of the vocabulary reaches these machines too. A publication
/// that took the universal key while this tagged one was written off holds it
/// under a key this one never claims, so recovering only its own keys would
/// leave both stored.
#[tokio::test]
async fn recovery_sees_a_scope_taken_through_the_other_form() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&ours.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let variant = format!("{owner}/entries/{entry}/{slot}.json");
    store.create_object(&variant, serde_json::to_vec(&ours.envelope).unwrap()).await.unwrap();
    // Reaches every machine, including the ones this artifact reaches, and it
    // took its key while this publication was written off.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/scopes/universal"),
            b"an artifact reaching everything".to_vec(),
        )
        .await
        .unwrap();

    let error = store
        .recover_after_expiry(&owner, &entry, &variant, &payload, &ours.envelope.digest().unwrap())
        .await
        .unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "the publication is told it lost, got {error:?}",
    );
    assert!(
        store.read_object_bounded(&variant, 4096).await.unwrap().is_none(),
        "and its artifact does not stay beside the one reaching the same machines",
    );
}
