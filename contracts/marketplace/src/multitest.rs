#![cfg(test)]
use crate::error::ContractError;
use crate::msg::{
    BidsResponse, CollectionBidResponse, CollectionBidsResponse, ExecuteMsg, QueryMsg,
};
use cosmwasm_std::{Addr, Empty};
use cw721::{Cw721QueryMsg, OwnerOfResponse};
use cw721_base::msg::{ExecuteMsg as Cw721ExecuteMsg, MintMsg};
use cw_multi_test::{BankSudo, Contract, ContractWrapper, Executor, SudoMsg as CwSudoMsg};
use sg_controllers::HooksResponse;
use sg_multi_test::StargazeApp;
use sg_std::StargazeMsgWrapper;

fn custom_mock_app() -> StargazeApp {
    StargazeApp::default()
}

pub fn contract_nft_marketplace() -> Box<dyn Contract<StargazeMsgWrapper>> {
    let contract = ContractWrapper::new(
        crate::execute::execute,
        crate::execute::instantiate,
        crate::query::query,
    )
    .with_sudo(crate::sudo::sudo);
    Box::new(contract)
}

pub fn contract_sg721() -> Box<dyn Contract<StargazeMsgWrapper>> {
    let contract = ContractWrapper::new(
        sg721::contract::execute,
        sg721::contract::instantiate,
        sg721::contract::query,
    );
    Box::new(contract)
}

#[cfg(test)]
mod tests {
    use crate::msg::{AsksResponse, BidResponse, CollectionsResponse, ParamResponse, SudoMsg};
    use crate::state::Bid;

    use super::*;
    use cosmwasm_std::{coin, coins, Coin, Decimal, Uint128};
    use sg721::msg::{InstantiateMsg as Sg721InstantiateMsg, RoyaltyInfoResponse};
    use sg721::state::CollectionInfo;
    use sg_multi_test::StargazeApp;
    use sg_std::NATIVE_DENOM;

    const TOKEN_ID: u32 = 123;
    const CREATION_FEE: u128 = 1_000_000_000;
    const INITIAL_BALANCE: u128 = 2000;
    // Governance parameters
    const TRADING_FEE_PERCENT: u32 = 2; // 2%
    const MIN_EXPIRY: u64 = 24 * 60 * 60; // 24 hours (in seconds)
    const MAX_EXPIRY: u64 = 180 * 24 * 60 * 60; // 6 months (in seconds)

    // Instantiates all needed contracts for testing
    fn setup_contracts(
        router: &mut StargazeApp,
        creator: &Addr,
    ) -> Result<(Addr, Addr), ContractError> {
        // Instantiate marketplace contract
        let marketplace_id = router.store_code(contract_nft_marketplace());
        let msg = crate::msg::InstantiateMsg {
            operators: vec!["operator".to_string()],
            trading_fee_percent: TRADING_FEE_PERCENT,
            ask_expiry: (MIN_EXPIRY, MAX_EXPIRY),
            bid_expiry: (MIN_EXPIRY, MAX_EXPIRY),
        };
        let nft_marketplace_addr = router
            .instantiate_contract(
                marketplace_id,
                creator.clone(),
                &msg,
                &[],
                "Marketplace",
                None,
            )
            .unwrap();

        // Setup media contract
        let sg721_id = router.store_code(contract_sg721());
        let msg = Sg721InstantiateMsg {
            name: String::from("Test Coin"),
            symbol: String::from("TEST"),
            minter: creator.to_string(),
            collection_info: CollectionInfo {
                creator: creator.to_string(),
                description: String::from("Stargaze Monkeys"),
                image: "ipfs://bafybeigi3bwpvyvsmnbj46ra4hyffcxdeaj6ntfk5jpic5mx27x6ih2qvq/images/1.png".to_string(),
                external_link: Some("https://example.com/external.html".to_string()),
                royalty_info: Some(RoyaltyInfoResponse {
                    payment_address: creator.to_string(),
                    share: Decimal::percent(10),
                }),
            },
        };
        let nft_contract_addr = router
            .instantiate_contract(
                sg721_id,
                creator.clone(),
                &msg,
                &coins(CREATION_FEE, NATIVE_DENOM),
                "NFT",
                None,
            )
            .unwrap();

        Ok((nft_marketplace_addr, nft_contract_addr))
    }

    // Intializes accounts with balances
    fn setup_accounts(router: &mut StargazeApp) -> Result<(Addr, Addr, Addr), ContractError> {
        let owner: Addr = Addr::unchecked("owner");
        let bidder: Addr = Addr::unchecked("bidder");
        let creator: Addr = Addr::unchecked("creator");
        let creator_funds: Vec<Coin> = coins(CREATION_FEE, NATIVE_DENOM);
        let funds: Vec<Coin> = coins(INITIAL_BALANCE, NATIVE_DENOM);
        router
            .sudo(CwSudoMsg::Bank({
                BankSudo::Mint {
                    to_address: owner.to_string(),
                    amount: funds.clone(),
                }
            }))
            .map_err(|err| println!("{:?}", err))
            .ok();
        router
            .sudo(CwSudoMsg::Bank({
                BankSudo::Mint {
                    to_address: bidder.to_string(),
                    amount: funds.clone(),
                }
            }))
            .map_err(|err| println!("{:?}", err))
            .ok();
        router
            .sudo(CwSudoMsg::Bank({
                BankSudo::Mint {
                    to_address: creator.to_string(),
                    amount: creator_funds.clone(),
                }
            }))
            .map_err(|err| println!("{:?}", err))
            .ok();

        // Check native balances
        let owner_native_balances = router.wrap().query_all_balances(owner.clone()).unwrap();
        assert_eq!(owner_native_balances, funds);
        let bidder_native_balances = router.wrap().query_all_balances(bidder.clone()).unwrap();
        assert_eq!(bidder_native_balances, funds);
        let creator_native_balances = router.wrap().query_all_balances(creator.clone()).unwrap();
        assert_eq!(creator_native_balances, creator_funds);

        Ok((owner, bidder, creator))
    }

    // Mints an NFT for a creator
    fn mint_nft_for_creator(router: &mut StargazeApp, creator: &Addr, nft_contract_addr: &Addr) {
        let mint_for_creator_msg = Cw721ExecuteMsg::Mint(MintMsg {
            token_id: TOKEN_ID.to_string(),
            owner: creator.clone().to_string(),
            token_uri: Some("https://starships.example.com/Starship/Enterprise.json".into()),
            extension: Empty {},
        });
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &mint_for_creator_msg,
            &[],
        );
        assert!(res.is_ok());
    }

    #[test]
    fn try_set_accept_bid() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (nft_marketplace_addr, nft_contract_addr) =
            setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &nft_contract_addr);

        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());

        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(110, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        // Should error on non-admin trying to update active state
        let update_ask_state = ExecuteMsg::UpdateAskState {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            active: false,
        };
        router
            .execute_contract(
                creator.clone(),
                nft_marketplace_addr.clone(),
                &update_ask_state,
                &[],
            )
            .unwrap_err();

        // Should not error on admin updating active state
        let res = router.execute_contract(
            Addr::unchecked("operator"),
            nft_marketplace_addr.clone(),
            &update_ask_state,
            &[],
        );
        assert!(res.is_ok());

        // Reset active state
        let update_ask_state = ExecuteMsg::UpdateAskState {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            active: true,
        };
        let res = router.execute_contract(
            Addr::unchecked("operator"),
            nft_marketplace_addr.clone(),
            &update_ask_state,
            &[],
        );
        assert!(res.is_ok());

        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder.clone(),
            nft_marketplace_addr.clone(),
            &set_bid_msg,
            &coins(100, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        // Check contract has been paid
        let contract_balances = router
            .wrap()
            .query_all_balances(nft_marketplace_addr.clone())
            .unwrap();
        assert_eq!(contract_balances, coins(100, NATIVE_DENOM));

        // Check creator hasn't been paid yet
        let creator_native_balances = router.wrap().query_all_balances(creator.clone()).unwrap();
        assert_eq!(creator_native_balances, vec![]);

        // Creator accepts bid
        let accept_bid_msg = ExecuteMsg::AcceptBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            bidder: bidder.to_string(),
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_marketplace_addr.clone(),
            &accept_bid_msg,
            &[],
        );
        assert!(res.is_ok());

        // Check money is transfered
        let creator_native_balances = router.wrap().query_all_balances(creator).unwrap();
        // 100  - 2 (fee)
        assert_eq!(creator_native_balances, coins(100 - 2, NATIVE_DENOM));
        let bidder_native_balances = router.wrap().query_all_balances(bidder.clone()).unwrap();
        assert_eq!(
            bidder_native_balances,
            coins(INITIAL_BALANCE - 100, NATIVE_DENOM)
        );

        // Check NFT is transferred
        let query_owner_msg = Cw721QueryMsg::OwnerOf {
            token_id: TOKEN_ID.to_string(),
            include_expired: None,
        };
        let res: OwnerOfResponse = router
            .wrap()
            .query_wasm_smart(nft_contract_addr, &query_owner_msg)
            .unwrap();
        assert_eq!(res.owner, bidder.to_string());

        // Check contract has zero balance
        let contract_balances = router
            .wrap()
            .query_all_balances(nft_marketplace_addr)
            .unwrap();
        assert_eq!(contract_balances, []);
    }

    #[test]
    fn check_bids_after_removing_ask() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (nft_marketplace_addr, nft_contract_addr) =
            setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &nft_contract_addr);

        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());

        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(110, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder,
            nft_marketplace_addr.clone(),
            &set_bid_msg,
            &coins(100, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        // Creator removes ask
        let remove_ask_msg = ExecuteMsg::RemoveAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_marketplace_addr.clone(),
            &remove_ask_msg,
            &[],
        );
        assert!(res.is_ok());

        // Check if bid has be removed
        let query_bids_msg = QueryMsg::Bids {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            start_after: None,
            limit: None,
        };
        let res: BidsResponse = router
            .wrap()
            .query_wasm_smart(nft_marketplace_addr, &query_bids_msg)
            .unwrap();
        assert_eq!(res.bids, vec![]);
    }

    #[test]
    fn try_update_ask() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, _, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (nft_marketplace_addr, nft_contract_addr) =
            setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &nft_contract_addr);

        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());

        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(110, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        let update_ask = ExecuteMsg::UpdateAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(200, NATIVE_DENOM),
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_marketplace_addr.clone(),
            &update_ask,
            &[],
        );
        assert!(res.is_ok());

        let update_ask = ExecuteMsg::UpdateAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(200, "bobo"),
        };
        router
            .execute_contract(
                creator.clone(),
                nft_marketplace_addr.clone(),
                &update_ask,
                &[],
            )
            .unwrap_err();

        let update_ask = ExecuteMsg::UpdateAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(0, NATIVE_DENOM),
        };
        router
            .execute_contract(creator.clone(), nft_marketplace_addr, &update_ask, &[])
            .unwrap_err();
    }

    #[test]
    fn try_query_asks() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, _, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (nft_marketplace_addr, nft_contract_addr) =
            setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &nft_contract_addr);

        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());

        // test before ask is made, without using pagination
        let query_asks_msg = QueryMsg::Asks {
            collection: nft_contract_addr.to_string(),
            start_after: None,
            limit: None,
        };
        let res: AsksResponse = router
            .wrap()
            .query_wasm_smart(nft_marketplace_addr.clone(), &query_asks_msg)
            .unwrap();
        assert_eq!(res.asks, vec![]);

        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(110, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        // test after ask is made
        let res: AsksResponse = router
            .wrap()
            .query_wasm_smart(nft_marketplace_addr.clone(), &query_asks_msg)
            .unwrap();
        assert_eq!(res.asks[0].token_id, TOKEN_ID);
        assert_eq!(res.asks[0].price.u128(), 110);

        // test pagination, starting when tokens exist
        let query_asks_msg = QueryMsg::Asks {
            collection: nft_contract_addr.to_string(),
            start_after: Some(TOKEN_ID - 1),
            limit: None,
        };
        let res: AsksResponse = router
            .wrap()
            .query_wasm_smart(nft_marketplace_addr.clone(), &query_asks_msg)
            .unwrap();
        assert_eq!(res.asks[0].token_id, TOKEN_ID);

        // test pagination, starting when token don't exist
        let query_asks_msg = QueryMsg::Asks {
            collection: nft_contract_addr.to_string(),
            start_after: Some(TOKEN_ID),
            limit: None,
        };
        let res: AsksResponse = router
            .wrap()
            .query_wasm_smart(nft_marketplace_addr.clone(), &query_asks_msg)
            .unwrap();
        assert_eq!(res.asks.len(), 0);

        // test listed collections query
        let res: CollectionsResponse = router
            .wrap()
            .query_wasm_smart(
                nft_marketplace_addr,
                &QueryMsg::ListedCollections {
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(res.collections[0], "contract1");
    }

    #[test]
    fn try_query_sorted_asks() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, _, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (marketplace, collection) = setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &collection);

        let mint_for_creator_msg = Cw721ExecuteMsg::Mint(MintMsg {
            token_id: (TOKEN_ID + 1).to_string(),
            owner: creator.clone().to_string(),
            token_uri: Some("https://starships.example.com/Starship/Enterprise.json".into()),
            extension: Empty {},
        });
        let res = router.execute_contract(
            creator.clone(),
            collection.clone(),
            &mint_for_creator_msg,
            &[],
        );
        assert!(res.is_ok());

        let mint_for_creator_msg = Cw721ExecuteMsg::Mint(MintMsg {
            token_id: (TOKEN_ID + 2).to_string(),
            owner: creator.clone().to_string(),
            token_uri: Some("https://starships.example.com/Starship/Enterprise.json".into()),
            extension: Empty {},
        });
        let res = router.execute_contract(
            creator.clone(),
            collection.clone(),
            &mint_for_creator_msg,
            &[],
        );
        assert!(res.is_ok());

        // Creator Authorizes NFTs
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: marketplace.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(creator.clone(), collection.clone(), &approve_msg, &[]);
        assert!(res.is_ok());
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: marketplace.to_string(),
            token_id: (TOKEN_ID + 1).to_string(),
            expires: None,
        };
        let res = router.execute_contract(creator.clone(), collection.clone(), &approve_msg, &[]);
        assert!(res.is_ok());
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: marketplace.to_string(),
            token_id: (TOKEN_ID + 2).to_string(),
            expires: None,
        };
        let res = router.execute_contract(creator.clone(), collection.clone(), &approve_msg, &[]);
        assert!(res.is_ok());

        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: collection.to_string(),
            token_id: TOKEN_ID,
            price: coin(110, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(creator.clone(), marketplace.clone(), &set_ask, &[]);
        assert!(res.is_ok());
        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: collection.to_string(),
            token_id: TOKEN_ID + 1,
            price: coin(109, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(creator.clone(), marketplace.clone(), &set_ask, &[]);
        assert!(res.is_ok());
        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: collection.to_string(),
            token_id: TOKEN_ID + 2,
            price: coin(111, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(creator.clone(), marketplace.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        let query_asks_msg = QueryMsg::AsksSortedByPrice {
            collection: collection.to_string(),
            limit: None,
        };
        let res: AsksResponse = router
            .wrap()
            .query_wasm_smart(marketplace, &query_asks_msg)
            .unwrap();
        assert_eq!(res.asks.len(), 3);
        assert_eq!(res.asks[0].price.u128(), 109u128);
        assert_eq!(res.asks[1].price.u128(), 110u128);
        assert_eq!(res.asks[2].price.u128(), 111u128);
    }

    #[test]
    fn try_query_sorted_bids() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (marketplace, collection) = setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &collection);

        let mint_for_creator_msg = Cw721ExecuteMsg::Mint(MintMsg {
            token_id: (TOKEN_ID + 1).to_string(),
            owner: creator.clone().to_string(),
            token_uri: Some("https://starships.example.com/Starship/Enterprise.json".into()),
            extension: Empty {},
        });
        let res = router.execute_contract(
            creator.clone(),
            collection.clone(),
            &mint_for_creator_msg,
            &[],
        );
        assert!(res.is_ok());

        let mint_for_creator_msg = Cw721ExecuteMsg::Mint(MintMsg {
            token_id: (TOKEN_ID + 2).to_string(),
            owner: creator.clone().to_string(),
            token_uri: Some("https://starships.example.com/Starship/Enterprise.json".into()),
            extension: Empty {},
        });
        let res = router.execute_contract(
            creator.clone(),
            collection.clone(),
            &mint_for_creator_msg,
            &[],
        );
        assert!(res.is_ok());

        // Creator Authorizes NFTs
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: marketplace.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(creator.clone(), collection.clone(), &approve_msg, &[]);
        assert!(res.is_ok());
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: marketplace.to_string(),
            token_id: (TOKEN_ID + 1).to_string(),
            expires: None,
        };
        let res = router.execute_contract(creator.clone(), collection.clone(), &approve_msg, &[]);
        assert!(res.is_ok());
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: marketplace.to_string(),
            token_id: (TOKEN_ID + 2).to_string(),
            expires: None,
        };
        let res = router.execute_contract(creator.clone(), collection.clone(), &approve_msg, &[]);
        assert!(res.is_ok());

        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: collection.to_string(),
            token_id: TOKEN_ID,
            price: coin(110, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(creator.clone(), marketplace.clone(), &set_ask, &[]);
        assert!(res.is_ok());
        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: collection.to_string(),
            token_id: TOKEN_ID + 1,
            price: coin(109, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(creator.clone(), marketplace.clone(), &set_ask, &[]);
        assert!(res.is_ok());
        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: collection.to_string(),
            token_id: TOKEN_ID + 2,
            price: coin(111, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(creator.clone(), marketplace.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: collection.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder.clone(),
            marketplace.clone(),
            &set_bid_msg,
            &coins(5, NATIVE_DENOM),
        );
        assert!(res.is_ok());
        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: collection.to_string(),
            token_id: TOKEN_ID + 1,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder.clone(),
            marketplace.clone(),
            &set_bid_msg,
            &coins(7, NATIVE_DENOM),
        );
        assert!(res.is_ok());
        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: collection.to_string(),
            token_id: TOKEN_ID + 2,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder,
            marketplace.clone(),
            &set_bid_msg,
            &coins(6, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        let query_bids_msg = QueryMsg::BidsSortedByPrice {
            collection: collection.to_string(),
            limit: None,
        };
        let res: BidsResponse = router
            .wrap()
            .query_wasm_smart(marketplace.clone(), &query_bids_msg)
            .unwrap();
        assert_eq!(res.bids.len(), 3);
        assert_eq!(res.bids[0].price.u128(), 5u128);
        assert_eq!(res.bids[1].price.u128(), 6u128);
        assert_eq!(res.bids[2].price.u128(), 7u128);

        // test adding another bid to an existing ask
        let bidder2: Addr = Addr::unchecked("bidder2");
        let funds: Vec<Coin> = coins(INITIAL_BALANCE, NATIVE_DENOM);
        router
            .sudo(CwSudoMsg::Bank({
                BankSudo::Mint {
                    to_address: bidder2.to_string(),
                    amount: funds,
                }
            }))
            .map_err(|err| println!("{:?}", err))
            .ok();

        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: collection.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder2,
            marketplace.clone(),
            &set_bid_msg,
            &coins(4, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        let res: BidsResponse = router
            .wrap()
            .query_wasm_smart(marketplace, &query_bids_msg)
            .unwrap();
        assert_eq!(res.bids.len(), 4);
        assert_eq!(res.bids[0].price.u128(), 4u128);
        assert_eq!(res.bids[1].price.u128(), 5u128);
        assert_eq!(res.bids[2].price.u128(), 6u128);
        assert_eq!(res.bids[3].price.u128(), 7u128);
    }

    #[test]
    fn try_query_bids() {
        let mut router = custom_mock_app();
        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();
        // Instantiate and configure contracts
        let (nft_marketplace_addr, nft_contract_addr) =
            setup_contracts(&mut router, &creator).unwrap();
        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &nft_contract_addr);
        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());
        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(110, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        // test before bid is made
        let query_bids_msg = QueryMsg::Bids {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            start_after: None,
            limit: None,
        };
        let res: BidsResponse = router
            .wrap()
            .query_wasm_smart(nft_marketplace_addr.clone(), &query_bids_msg)
            .unwrap();
        assert_eq!(res.bids, vec![]);

        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder,
            nft_marketplace_addr.clone(),
            &set_bid_msg,
            &coins(100, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        let res: BidsResponse = router
            .wrap()
            .query_wasm_smart(nft_marketplace_addr, &query_bids_msg)
            .unwrap();
        assert_eq!(res.bids[0].token_id, TOKEN_ID);
        assert_eq!(res.bids[0].price.u128(), 100u128);
    }

    #[test]
    fn auto_accept_bid() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (nft_marketplace_addr, nft_contract_addr) =
            setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &nft_contract_addr);

        // An ask is made by the creator, but fails because NFT is not authorized
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(100, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_err());

        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());

        // Now set_ask succeeds
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        println!("{:?}", res);
        assert!(res.is_ok());

        // Bidder makes bid with a random token in the same amount as the ask
        router
            .sudo(CwSudoMsg::Bank({
                BankSudo::Mint {
                    to_address: bidder.to_string(),
                    amount: coins(1000, "random"),
                }
            }))
            .map_err(|err| println!("{:?}", err))
            .ok();
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        router
            .execute_contract(
                bidder.clone(),
                nft_marketplace_addr.clone(),
                &set_bid_msg,
                &coins(100, "random"),
            )
            .unwrap_err();

        // Bidder makes bid that meets the ask criteria
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router
            .execute_contract(
                bidder.clone(),
                nft_marketplace_addr,
                &set_bid_msg,
                &coins(100, NATIVE_DENOM),
            )
            .unwrap();

        // Bid is accepted, sale has been finalized
        assert_eq!("sale_finalized", res.events[1].attributes[1].value);

        // Check money is transfered
        let creator_native_balances = router.wrap().query_all_balances(creator).unwrap();
        // 100  - 2 (fee)
        assert_eq!(creator_native_balances, coins(100 - 2, NATIVE_DENOM));
        let bidder_native_balances = router.wrap().query_all_balances(bidder.clone()).unwrap();
        assert_eq!(
            bidder_native_balances,
            vec![
                coin(1000, "random"),
                coin(INITIAL_BALANCE - 100, NATIVE_DENOM),
            ]
        );

        // Check NFT is transferred
        let query_owner_msg = Cw721QueryMsg::OwnerOf {
            token_id: TOKEN_ID.to_string(),
            include_expired: None,
        };
        let res: OwnerOfResponse = router
            .wrap()
            .query_wasm_smart(nft_contract_addr, &query_owner_msg)
            .unwrap();
        assert_eq!(res.owner, bidder.to_string());
    }

    #[test]
    fn remove_bid_refund() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (nft_marketplace_addr, nft_contract_addr) =
            setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &nft_contract_addr);

        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());

        // An asking price is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(110, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };

        let res = router.execute_contract(
            bidder.clone(),
            nft_marketplace_addr.clone(),
            &set_bid_msg,
            &coins(100, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        // Bidder sent bid money
        let bidder_native_balances = router.wrap().query_all_balances(bidder.clone()).unwrap();
        assert_eq!(
            bidder_native_balances,
            coins(INITIAL_BALANCE - 100, NATIVE_DENOM)
        );

        // Contract has been paid
        let contract_balances = router
            .wrap()
            .query_all_balances(nft_marketplace_addr.clone())
            .unwrap();
        assert_eq!(contract_balances, coins(100, NATIVE_DENOM));

        // Bidder removes bid
        let remove_bid_msg = ExecuteMsg::RemoveBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
        };
        let res =
            router.execute_contract(bidder.clone(), nft_marketplace_addr, &remove_bid_msg, &[]);
        assert!(res.is_ok());

        // Bidder has money back
        let bidder_native_balances = router.wrap().query_all_balances(bidder).unwrap();
        assert_eq!(bidder_native_balances, coins(INITIAL_BALANCE, NATIVE_DENOM));
    }

    #[test]
    fn new_bid_refund() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (nft_marketplace_addr, nft_contract_addr) =
            setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &nft_contract_addr);

        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());

        // An ask is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(200, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder.clone(),
            nft_marketplace_addr.clone(),
            &set_bid_msg,
            &coins(100, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        // Bidder sent bid money
        let bidder_native_balances = router.wrap().query_all_balances(bidder.clone()).unwrap();
        assert_eq!(
            bidder_native_balances,
            coins(INITIAL_BALANCE - 100, NATIVE_DENOM)
        );

        // Contract has been paid
        let contract_balances = router
            .wrap()
            .query_all_balances(nft_marketplace_addr.clone())
            .unwrap();
        assert_eq!(contract_balances, coins(100, NATIVE_DENOM));

        // Bidder makes higher bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder.clone(),
            nft_marketplace_addr.clone(),
            &set_bid_msg,
            &coins(150, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        // Bidder has money back
        let bidder_native_balances = router.wrap().query_all_balances(bidder.clone()).unwrap();
        assert_eq!(
            bidder_native_balances,
            coins(INITIAL_BALANCE - 150, NATIVE_DENOM)
        );

        // Contract has been paid
        let contract_balances = router
            .wrap()
            .query_all_balances(nft_marketplace_addr.clone())
            .unwrap();
        assert_eq!(contract_balances, coins(150, NATIVE_DENOM));

        // Check new bid has been saved
        let query_bid_msg = QueryMsg::Bid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            bidder: bidder.to_string(),
        };
        let bid = Bid {
            collection: nft_contract_addr,
            token_id: TOKEN_ID,
            bidder,
            price: Uint128::from(150u128),
            expires: (router.block_info().time.plus_seconds(MIN_EXPIRY + 1)),
        };

        let res: BidResponse = router
            .wrap()
            .query_wasm_smart(nft_marketplace_addr, &query_bid_msg)
            .unwrap();
        assert_eq!(res.bid, Some(bid));
    }

    #[test]
    fn try_royalties() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (curator, bidder, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate marketplace contract
        let marketplace_id = router.store_code(contract_nft_marketplace());
        let msg = crate::msg::InstantiateMsg {
            operators: vec!["operator".to_string()],
            trading_fee_percent: TRADING_FEE_PERCENT,
            ask_expiry: (MIN_EXPIRY, MAX_EXPIRY),
            bid_expiry: (MIN_EXPIRY, MAX_EXPIRY),
        };
        let nft_marketplace_addr = router
            .instantiate_contract(
                marketplace_id,
                creator.clone(),
                &msg,
                &[],
                "Marketplace",
                None,
            )
            .unwrap();

        // Setup media contract with 10% royalties to a curator
        let sg721_id = router.store_code(contract_sg721());
        let msg = sg721::msg::InstantiateMsg {
            name: String::from("Test Coin"),
            symbol: String::from("TEST"),
            minter: creator.to_string(),
            collection_info: CollectionInfo {
                creator: creator.to_string(),
                description: String::from("Stargaze Monkeys"),
                image: "https://example.com/image.png".to_string(),
                external_link: Some("https://example.com/external.html".to_string()),
                royalty_info: Some(RoyaltyInfoResponse {
                    payment_address: curator.to_string(),
                    share: Decimal::percent(10),
                }),
            },
        };
        let nft_contract_addr = router
            .instantiate_contract(
                sg721_id,
                creator.clone(),
                &msg,
                &coins(CREATION_FEE, NATIVE_DENOM),
                "NFT",
                None,
            )
            .unwrap();

        // Mint NFT for creator
        let mint_for_creator_msg = Cw721ExecuteMsg::Mint(MintMsg {
            token_id: TOKEN_ID.to_string(),
            owner: creator.to_string(),
            token_uri: Some("https://starships.example.com/Starship/Enterprise.json".into()),
            extension: Empty {},
        });
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &mint_for_creator_msg,
            &[],
        );
        assert!(res.is_ok());

        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: nft_marketplace_addr.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(
            creator.clone(),
            nft_contract_addr.clone(),
            &approve_msg,
            &[],
        );
        assert!(res.is_ok());

        // An ask is made by the creator
        let set_ask = ExecuteMsg::SetAsk {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            price: coin(100, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res =
            router.execute_contract(creator.clone(), nft_marketplace_addr.clone(), &set_ask, &[]);
        assert!(res.is_ok());

        // Bidder makes bid
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: nft_contract_addr.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder.clone(),
            nft_marketplace_addr,
            &set_bid_msg,
            &coins(100, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        // Check money is transfered correctly and royalties paid
        let curator_native_balances = router.wrap().query_all_balances(curator).unwrap();
        assert_eq!(
            curator_native_balances,
            coins(INITIAL_BALANCE + 10, NATIVE_DENOM)
        );
        let creator_native_balances = router.wrap().query_all_balances(creator).unwrap();
        // 100 - 10 (royalties) - 2 (fee)
        assert_eq!(creator_native_balances, coins(100 - 10 - 2, NATIVE_DENOM));
        let bidder_native_balances = router.wrap().query_all_balances(bidder.clone()).unwrap();
        assert_eq!(
            bidder_native_balances,
            coins(INITIAL_BALANCE - 100, NATIVE_DENOM)
        );

        // Check NFT is transferred
        let query_owner_msg = Cw721QueryMsg::OwnerOf {
            token_id: TOKEN_ID.to_string(),
            include_expired: None,
        };
        let res: OwnerOfResponse = router
            .wrap()
            .query_wasm_smart(nft_contract_addr, &query_owner_msg)
            .unwrap();
        assert_eq!(res.owner, bidder.to_string());
    }

    #[test]
    fn try_sudo_update_config() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, _, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (marketplace, _) = setup_contracts(&mut router, &creator).unwrap();

        let update_config_msg = SudoMsg::UpdateParams {
            trading_fee_percent: Some(5),
            ask_expiry: Some((1, 2)),
            bid_expiry: None,
            operators: Some(vec!["operator".to_string()]),
        };
        let res = router.wasm_sudo(marketplace.clone(), &update_config_msg);
        assert!(res.is_ok());

        let query_params_msg = QueryMsg::Params {};
        let res: ParamResponse = router
            .wrap()
            .query_wasm_smart(marketplace, &query_params_msg)
            .unwrap();
        assert_eq!(res.params.trading_fee_percent, 5);
        assert_eq!(res.params.ask_expiry, (1, 2));
        assert_eq!(res.params.operators, vec!["operator".to_string()]);
    }

    #[test]
    fn try_add_remove_hooks() {
        let mut router = custom_mock_app();
        // Setup intial accounts
        let (_owner, _, creator) = setup_accounts(&mut router).unwrap();
        // Instantiate and configure contracts
        let (marketplace, _) = setup_contracts(&mut router, &creator).unwrap();

        let add_hook_msg = SudoMsg::AddSaleFinalizedHook {
            hook: "hook".to_string(),
        };
        let res = router.wasm_sudo(marketplace.clone(), &add_hook_msg);
        assert!(res.is_ok());

        let query_hooks_msg = QueryMsg::SaleFinalizedHooks {};
        let res: HooksResponse = router
            .wrap()
            .query_wasm_smart(marketplace.clone(), &query_hooks_msg)
            .unwrap();
        assert_eq!(res.hooks, vec!["hook".to_string()]);

        let remove_hook_msg = SudoMsg::RemoveSaleFinalizedHook {
            hook: "hook".to_string(),
        };
        let res = router.wasm_sudo(marketplace.clone(), &remove_hook_msg);
        assert!(res.is_ok());

        let res: HooksResponse = router
            .wrap()
            .query_wasm_smart(marketplace, &query_hooks_msg)
            .unwrap();
        assert!(res.hooks.is_empty());
    }

    #[test]
    fn try_hook_was_run() {
        let mut router = custom_mock_app();
        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();
        // Instantiate and configure contracts
        let (marketplace, collection) = setup_contracts(&mut router, &creator).unwrap();

        let add_hook_msg = SudoMsg::AddSaleFinalizedHook {
            hook: "hook".to_string(),
        };
        let _res = router.wasm_sudo(marketplace.clone(), &add_hook_msg);

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &collection);

        // An ask is made by the creator, but fails because NFT is not authorized
        let set_ask = ExecuteMsg::SetAsk {
            collection: collection.to_string(),
            token_id: TOKEN_ID,
            price: coin(100, NATIVE_DENOM),
            funds_recipient: None,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        // Creator Authorizes NFT
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: marketplace.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(creator.clone(), collection.clone(), &approve_msg, &[]);
        assert!(res.is_ok());
        // Now set_ask succeeds
        let res = router.execute_contract(creator.clone(), marketplace.clone(), &set_ask, &[]);
        assert!(res.is_ok());
        // Bidder makes bid that meets the ask criteria
        let set_bid_msg = ExecuteMsg::SetBid {
            collection: collection.to_string(),
            token_id: TOKEN_ID,
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        // Error because the "hook" contract is not deployed
        let _err = router
            .execute_contract(bidder, marketplace, &set_bid_msg, &coins(100, NATIVE_DENOM))
            .unwrap_err();

        // If the bid is accepted, the sale would be finalized
        // assert_eq!("sale_finalized", res.events[1].attributes[1].value);
    }

    #[test]
    fn try_collection_bids() {
        let mut router = custom_mock_app();

        // Setup intial accounts
        let (_owner, bidder, creator) = setup_accounts(&mut router).unwrap();

        // Instantiate and configure contracts
        let (marketplace, collection) = setup_contracts(&mut router, &creator).unwrap();

        // Mint NFT for creator
        mint_nft_for_creator(&mut router, &creator, &collection);

        // Creator Authorizes NFTs
        let approve_msg = Cw721ExecuteMsg::<Empty>::Approve {
            spender: marketplace.to_string(),
            token_id: TOKEN_ID.to_string(),
            expires: None,
        };
        let res = router.execute_contract(creator.clone(), collection.clone(), &approve_msg, &[]);
        assert!(res.is_ok());

        // A collection bid is made by the bidder
        let set_collection_bid = ExecuteMsg::SetCollectionBid {
            collection: collection.to_string(),
            expires: router.block_info().time.plus_seconds(MIN_EXPIRY + 1),
        };
        let res = router.execute_contract(
            bidder.clone(),
            marketplace.clone(),
            &set_collection_bid,
            &coins(150, NATIVE_DENOM),
        );
        assert!(res.is_ok());

        // test querying a single collection bid
        let query_collection_bid = QueryMsg::CollectionBid {
            collection: collection.to_string(),
            bidder: bidder.to_string(),
        };
        let res: CollectionBidResponse = router
            .wrap()
            .query_wasm_smart(marketplace.clone(), &query_collection_bid)
            .unwrap();
        assert_eq!(res.bid.unwrap().price.u128(), 150u128);

        // test querying all collection bids by bidder
        let query_collection_bids = QueryMsg::CollectionBidsByBidder {
            bidder: bidder.to_string(),
        };
        let res: CollectionBidsResponse = router
            .wrap()
            .query_wasm_smart(marketplace.clone(), &query_collection_bids)
            .unwrap();
        assert_eq!(res.bids[0].price.u128(), 150u128);

        // test querying all sorted collection bids by bidder
        let query_sorted_collection_bids = QueryMsg::CollectionBidsSortedByPrice {
            collection: collection.to_string(),
            limit: Some(10),
        };
        let res: CollectionBidsResponse = router
            .wrap()
            .query_wasm_smart(marketplace.clone(), &query_sorted_collection_bids)
            .unwrap();
        assert_eq!(res.bids[0].price.u128(), 150u128);

        // A collection bid is accepted
        let accept_collection_bid = ExecuteMsg::AcceptCollectionBid {
            collection: collection.to_string(),
            token_id: TOKEN_ID,
            bidder: bidder.to_string(),
        };
        let res =
            router.execute_contract(creator.clone(), marketplace, &accept_collection_bid, &[]);
        assert!(res.is_ok());
    }
}
