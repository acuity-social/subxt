// Copyright 2019-2021 Parity Technologies (UK) Ltd.
// This file is part of subxt.
//
// subxt is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// subxt is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with subxt.  If not, see <http://www.gnu.org/licenses/>.

use crate::{
    node_runtime::{
        balances,
        runtime_types,
        system,
        DefaultConfig,
    },
    test_context,
};
use codec::Decode;
use sp_core::{
    sr25519::Pair,
    Pair as _,
};
use sp_keyring::AccountKeyring;
use subxt::{
    extrinsic::{
        PairSigner,
        Signer,
    },
    Error,
    EventSubscription,
    PalletError,
    RuntimeError,
};

#[async_std::test]
async fn tx_basic_transfer() {
    let alice = PairSigner::<DefaultConfig, _>::new(AccountKeyring::Alice.pair());
    let bob = PairSigner::<DefaultConfig, _>::new(AccountKeyring::Bob.pair());
    let bob_address = bob.account_id().clone().into();
    let cxt = test_context().await;
    let api = &cxt.api;

    let alice_pre = api
        .storage()
        .system()
        .account(alice.account_id().clone().into(), None)
        .await
        .unwrap();
    let bob_pre = api
        .storage()
        .system()
        .account(bob.account_id().clone().into(), None)
        .await
        .unwrap();

    let result = api
        .tx()
        .balances()
        .transfer(bob_address, 10_000)
        .sign_and_submit_then_watch(&alice)
        .await
        .unwrap();
    let event = result
        .find_event::<balances::events::Transfer>()
        .unwrap()
        .unwrap();
    let _extrinsic_success = result
        .find_event::<system::events::ExtrinsicSuccess>()
        .expect("Failed to decode ExtrinisicSuccess".into())
        .expect("Failed to find ExtrinisicSuccess");

    let expected_event = balances::events::Transfer(
        alice.account_id().clone(),
        bob.account_id().clone(),
        10_000,
    );
    assert_eq!(event, expected_event);

    let alice_post = api
        .storage()
        .system()
        .account(alice.account_id().clone().into(), None)
        .await
        .unwrap();
    let bob_post = api
        .storage()
        .system()
        .account(bob.account_id().clone().into(), None)
        .await
        .unwrap();

    assert!(alice_pre.data.free - 10_000 >= alice_post.data.free);
    assert_eq!(bob_pre.data.free + 10_000, bob_post.data.free);
}

#[async_std::test]
async fn storage_total_issuance() {
    let cxt = test_context().await;
    let total_issuance = cxt
        .api
        .storage()
        .balances()
        .total_issuance(None)
        .await
        .unwrap();
    assert_ne!(total_issuance, 0);
}

#[async_std::test]
async fn storage_balance_lock() -> Result<(), subxt::Error> {
    let bob = PairSigner::<DefaultConfig, _>::new(AccountKeyring::Bob.pair());
    let charlie = AccountKeyring::Charlie.to_account_id();
    let cxt = test_context().await;

    let result = cxt
        .api
        .tx()
        .staking()
        .bond(
            charlie.into(),
            100_000_000_000_000,
            runtime_types::pallet_staking::RewardDestination::Stash,
        )
        .sign_and_submit_then_watch(&bob)
        .await?;

    let success = result.find_event::<system::events::ExtrinsicSuccess>()?;
    assert!(success.is_some(), "No ExtrinsicSuccess Event found");

    let locks = cxt
        .api
        .storage()
        .balances()
        .locks(AccountKeyring::Bob.to_account_id(), None)
        .await?;

    assert_eq!(
        locks.0,
        vec![runtime_types::pallet_balances::BalanceLock {
            id: *b"staking ",
            amount: 100_000_000_000_000,
            reasons: runtime_types::pallet_balances::Reasons::All,
        }]
    );

    Ok(())
}

#[async_std::test]
async fn transfer_error() {
    env_logger::try_init().ok();
    let alice = PairSigner::<DefaultConfig, _>::new(AccountKeyring::Alice.pair());
    let alice_addr = alice.account_id().clone().into();
    let hans = PairSigner::<DefaultConfig, _>::new(Pair::generate().0);
    let hans_address = hans.account_id().clone().into();
    let cxt = test_context().await;

    cxt.api
        .tx()
        .balances()
        .transfer(hans_address, 100_000_000_000_000_000)
        .sign_and_submit_then_watch(&alice)
        .await
        .unwrap();

    let res = cxt
        .api
        .tx()
        .balances()
        .transfer(alice_addr, 100_000_000_000_000_000)
        .sign_and_submit_then_watch(&hans)
        .await;

    if let Err(Error::Runtime(RuntimeError::Module(error))) = res {
        let error2 = PalletError {
            pallet: "Balances".into(),
            error: "InsufficientBalance".into(),
            description: vec!["Balance too low to send value".to_string()],
        };
        assert_eq!(error, error2);
    } else {
        panic!("expected an error");
    }
}

#[async_std::test]
async fn transfer_subscription() {
    env_logger::try_init().ok();
    let alice = PairSigner::<DefaultConfig, _>::new(AccountKeyring::Alice.pair());
    let bob = AccountKeyring::Bob.to_account_id();
    let bob_addr = bob.clone().into();
    let cxt = test_context().await;
    let sub = cxt.client().rpc().subscribe_events().await.unwrap();
    let decoder = cxt.client().events_decoder();
    let mut sub = EventSubscription::<DefaultConfig>::new(sub, &decoder);
    sub.filter_event::<balances::events::Transfer>();

    cxt.api
        .tx()
        .balances()
        .transfer(bob_addr, 10_000)
        .sign_and_submit_then_watch(&alice)
        .await
        .unwrap();

    let raw = sub.next().await.unwrap().unwrap();
    let event = balances::events::Transfer::decode(&mut &raw.data[..]).unwrap();
    assert_eq!(
        event,
        balances::events::Transfer(alice.account_id().clone(), bob.clone(), 10_000,)
    );
}

#[async_std::test]
async fn constant_existential_deposit() {
    let cxt = test_context().await;
    let balances_metadata = cxt.client().metadata().pallet("Balances").unwrap();
    let constant_metadata = balances_metadata.constant("ExistentialDeposit").unwrap();
    let existential_deposit = u128::decode(&mut &constant_metadata.value[..]).unwrap();
    assert_eq!(existential_deposit, 100_000_000_000_000);
}
