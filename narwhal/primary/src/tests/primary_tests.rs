// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use super::{NetworkModel, Primary, PrimaryReceiverHandler, CHANNEL_CAPACITY};
use crate::{common::create_db_stores, metrics::PrimaryChannelMetrics};
use arc_swap::ArcSwap;
use config::Parameters;
use consensus::{dag::Dag, metrics::ConsensusMetrics};
use crypto::PublicKey;
use fastcrypto::{traits::KeyPair, Hash};
use itertools::Itertools;
use node::NodeStorage;
use prometheus::Registry;
use std::{collections::BTreeSet, num::NonZeroUsize, sync::Arc, time::Duration};
use test_utils::{temp_dir, CommitteeFixture};
use tokio::sync::watch;
use types::{Certificate, FetchCertificatesRequest, PrimaryToPrimary, ReconfigureNotification};
use worker::{metrics::initialise_metrics, Worker};

#[tokio::test]
async fn get_network_peers_from_admin_server() {
    // telemetry_subscribers::init_for_testing();
    let primary_1_parameters = Parameters {
        batch_size: 200, // Two transactions.
        ..Parameters::default()
    };
    let fixture = CommitteeFixture::builder().randomize_ports(true).build();
    let committee = fixture.committee();
    let worker_cache = fixture.shared_worker_cache();
    let authority_1 = fixture.authorities().next().unwrap();
    let name_1 = authority_1.public_key();
    let signer_1 = authority_1.keypair().copy();

    let worker_id = 0;
    let worker_1_keypair = authority_1.worker(worker_id).keypair().copy();

    // Make the data store.
    let store = NodeStorage::reopen(temp_dir());

    let (tx_new_certificates, rx_new_certificates) = types::metered_channel::channel(
        CHANNEL_CAPACITY,
        &prometheus::IntGauge::new(
            PrimaryChannelMetrics::NAME_NEW_CERTS,
            PrimaryChannelMetrics::DESC_NEW_CERTS,
        )
        .unwrap(),
    );
    let (tx_feedback, rx_feedback) = types::metered_channel::channel(
        CHANNEL_CAPACITY,
        &prometheus::IntGauge::new(
            PrimaryChannelMetrics::NAME_COMMITTED_CERTS,
            PrimaryChannelMetrics::DESC_COMMITTED_CERTS,
        )
        .unwrap(),
    );
    let initial_committee = ReconfigureNotification::NewEpoch(committee.clone());
    let (tx_reconfigure, _rx_reconfigure) = watch::channel(initial_committee);
    let consensus_metrics = Arc::new(ConsensusMetrics::new(&Registry::new()));

    // Spawn Primary 1
    Primary::spawn(
        name_1.clone(),
        signer_1,
        authority_1.network_keypair().copy(),
        Arc::new(ArcSwap::from_pointee(committee.clone())),
        worker_cache.clone(),
        primary_1_parameters.clone(),
        store.header_store.clone(),
        store.certificate_store.clone(),
        store.proposer_store.clone(),
        store.payload_store.clone(),
        store.vote_digest_store.clone(),
        /* tx_consensus */ tx_new_certificates,
        /* rx_consensus */ rx_feedback,
        /* dag */
        Some(Arc::new(
            Dag::new(&committee, rx_new_certificates, consensus_metrics).1,
        )),
        NetworkModel::Asynchronous,
        tx_reconfigure,
        tx_feedback,
        &Registry::new(),
        None,
    );

    // Wait for tasks to start
    tokio::time::sleep(Duration::from_secs(1)).await;

    let registry_1 = Registry::new();
    let metrics_1 = initialise_metrics(&registry_1);

    let worker_1_parameters = Parameters {
        batch_size: 200, // Two transactions.
        ..Parameters::default()
    };

    // Spawn a `Worker` instance for primary 1.
    Worker::spawn(
        name_1,
        worker_1_keypair.copy(),
        worker_id,
        Arc::new(ArcSwap::from_pointee(committee.clone())),
        worker_cache.clone(),
        worker_1_parameters.clone(),
        store.batch_store,
        metrics_1,
    );

    // Test getting all known peers for primary 1
    let resp = reqwest::get(format!(
        "http://127.0.0.1:{}/known_peers",
        primary_1_parameters
            .network_admin_server
            .primary_network_admin_server_port
    ))
    .await
    .unwrap()
    .json::<Vec<String>>()
    .await
    .unwrap();

    // Assert we returned 19 peers (3 other primaries + 4 workers + 4*3 other workers)
    assert_eq!(19, resp.len());

    // Test getting all connected peers for primary 1
    let resp = reqwest::get(format!(
        "http://127.0.0.1:{}/peers",
        primary_1_parameters
            .network_admin_server
            .primary_network_admin_server_port
    ))
    .await
    .unwrap()
    .json::<Vec<String>>()
    .await
    .unwrap();

    // Assert we returned 1 peers (only 1 worker spawned)
    assert_eq!(1, resp.len());

    let authority_2 = fixture.authorities().nth(1).unwrap();
    let name_2 = authority_2.public_key();
    let signer_2 = authority_2.keypair().copy();

    let primary_2_parameters = Parameters {
        batch_size: 200, // Two transactions.
        ..Parameters::default()
    };

    // TODO: Rework test-utils so that macro can be used for the channels below.
    let (tx_new_certificates_2, rx_new_certificates_2) = types::metered_channel::channel(
        CHANNEL_CAPACITY,
        &prometheus::IntGauge::new(
            PrimaryChannelMetrics::NAME_NEW_CERTS,
            PrimaryChannelMetrics::DESC_NEW_CERTS,
        )
        .unwrap(),
    );
    let (tx_feedback_2, rx_feedback_2) = types::metered_channel::channel(
        CHANNEL_CAPACITY,
        &prometheus::IntGauge::new(
            PrimaryChannelMetrics::NAME_COMMITTED_CERTS,
            PrimaryChannelMetrics::DESC_COMMITTED_CERTS,
        )
        .unwrap(),
    );
    let initial_committee = ReconfigureNotification::NewEpoch(committee.clone());
    let (tx_reconfigure_2, _rx_reconfigure_2) = watch::channel(initial_committee);
    let consensus_metrics = Arc::new(ConsensusMetrics::new(&Registry::new()));

    // Spawn Primary 2
    Primary::spawn(
        name_2.clone(),
        signer_2,
        authority_2.network_keypair().copy(),
        Arc::new(ArcSwap::from_pointee(committee.clone())),
        worker_cache.clone(),
        primary_2_parameters.clone(),
        store.header_store.clone(),
        store.certificate_store.clone(),
        store.proposer_store.clone(),
        store.payload_store.clone(),
        store.vote_digest_store.clone(),
        /* tx_consensus */ tx_new_certificates_2,
        /* rx_consensus */ rx_feedback_2,
        /* dag */
        Some(Arc::new(
            Dag::new(&committee, rx_new_certificates_2, consensus_metrics).1,
        )),
        NetworkModel::Asynchronous,
        tx_reconfigure_2,
        tx_feedback_2,
        &Registry::new(),
        None,
    );

    // Wait for tasks to start
    tokio::time::sleep(Duration::from_secs(1)).await;

    let primary_1_peer_id = hex::encode(authority_1.network_keypair().copy().public().0.as_bytes());
    let primary_2_peer_id = hex::encode(authority_2.network_keypair().copy().public().0.as_bytes());
    let worker_1_peer_id = hex::encode(worker_1_keypair.copy().public().0.as_bytes());

    // Test getting all connected peers for primary 1
    let resp = reqwest::get(format!(
        "http://127.0.0.1:{}/peers",
        primary_1_parameters
            .network_admin_server
            .primary_network_admin_server_port
    ))
    .await
    .unwrap()
    .json::<Vec<String>>()
    .await
    .unwrap();

    // Assert we returned 2 peers (1 other primary spawned + 1 worker spawned)
    assert_eq!(2, resp.len());

    // Assert peer ids are correct
    let expected_peer_ids = vec![&primary_2_peer_id, &worker_1_peer_id];
    assert!(expected_peer_ids.iter().all(|e| resp.contains(e)));

    // Test getting all connected peers for primary 2
    let resp = reqwest::get(format!(
        "http://127.0.0.1:{}/peers",
        primary_2_parameters
            .network_admin_server
            .primary_network_admin_server_port
    ))
    .await
    .unwrap()
    .json::<Vec<String>>()
    .await
    .unwrap();

    // Assert we returned 2 peers (1 other primary spawned + 1 other worker)
    assert_eq!(2, resp.len());

    // Assert peer ids are correct
    let expected_peer_ids = vec![&primary_1_peer_id, &worker_1_peer_id];
    assert!(expected_peer_ids.iter().all(|e| resp.contains(e)));
}

#[tokio::test]
async fn test_fetch_certificates_handler() {
    let fixture = CommitteeFixture::builder()
        .randomize_ports(true)
        .committee_size(NonZeroUsize::new(4).unwrap())
        .build();
    let committee = fixture.committee();

    let (tx_primary_messages, _) = test_utils::test_channel!(1);
    let (tx_helper_requests, _) = test_utils::test_channel!(1);
    let (tx_availability_responses, _) = test_utils::test_channel!(1);
    let (_, certificate_store, _) = create_db_stores();
    let handler = PrimaryReceiverHandler {
        tx_primary_messages,
        tx_helper_requests,
        tx_availability_responses,
        certificate_store: certificate_store.clone(),
    };

    // Generate headers and certificates in successive rounds
    let genesis_certs: Vec<_> = Certificate::genesis(&committee);
    for cert in genesis_certs.iter() {
        certificate_store
            .write(cert.clone())
            .expect("Writing certificate to store failed");
    }

    let mut current_round: Vec<_> = genesis_certs.into_iter().map(|cert| cert.header).collect();
    let mut headers = vec![];
    let total_rounds = 4;
    for i in 0..total_rounds {
        let parents: BTreeSet<_> = current_round
            .into_iter()
            .map(|header| fixture.certificate(&header).digest())
            .collect();
        (_, current_round) = fixture.headers_round(i, &parents);
        headers.extend(current_round.clone());
    }

    let total_authorities = fixture.authorities().count();
    let total_certificates = total_authorities * total_rounds as usize;
    // Create certificates test data.
    let mut certificates = vec![];
    for header in headers.into_iter() {
        certificates.push(fixture.certificate(&header));
    }
    assert_eq!(certificates.len(), total_certificates);
    assert_eq!(16, total_certificates);

    // Populate certificate store such that each authority has the following rounds:
    // Authority 0: 0 1
    // Authority 1: 0 1 2
    // Authority 2: 0 1 2 3
    // Authority 3: 0 1 2 3 4
    let mut authorities = Vec::<PublicKey>::new();
    for i in 0..total_authorities {
        authorities.push(certificates[i].header.author.clone());
        for j in 0..=i {
            let cert = certificates[i + j * total_authorities].clone();
            assert_eq!(&cert.header.author, authorities.last().unwrap());
            certificate_store
                .write(cert)
                .expect("Writing certificate to store failed");
        }
    }

    // Fetch all certificates above rounds [1, 1, 2, 2] for the corresponding authorities.
    let req = FetchCertificatesRequest {
        progression: vec![1u64, 1, 3, 3]
            .into_iter()
            .zip(authorities.clone().into_iter())
            .collect_vec(),
        max_items: 5,
    };
    let resp = handler
        .fetch_certificates(anemo::Request::new(req.clone()))
        .await
        .unwrap()
        .into_body();
    assert_eq!(
        resp.certificates
            .iter()
            .map(|cert| cert.round())
            .collect_vec(),
        vec![2, 4]
    );
}
