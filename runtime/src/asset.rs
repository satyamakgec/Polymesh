use crate::{
    balances,
    constants::*,
    general_tm, identity, percentage_tm,
    registry::{self, RegistryEntry, TokenType},
    utils,
};
use codec::Encode;
use core::result::Result as StdResult;
use primitives::{IdentityId, Key};
use rstd::{convert::TryFrom, prelude::*};
use session;
use sr_primitives::traits::{CheckedAdd, CheckedSub};
use srml_support::{
    decl_event, decl_module, decl_storage,
    dispatch::Result,
    ensure,
    traits::{Currency, ExistenceRequirement, WithdrawReason},
};
use system::{self, ensure_signed};

/// The module's configuration trait.
pub trait Trait:
    system::Trait
    + general_tm::Trait
    + percentage_tm::Trait
    + utils::Trait
    + balances::Trait
    + identity::Trait
    + session::Trait
    + registry::Trait
{
    /// The overarching event type.
    type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;
    //type TokenBalance: Parameter + Member + SimpleArithmetic + Codec + Default + Copy + As<usize> + As<u64>;
    type Currency: Currency<Self::AccountId>;
}

// struct to store the token details
#[derive(codec::Encode, codec::Decode, Default, Clone, PartialEq, Debug)]
pub struct SecurityToken<U> {
    pub name: Vec<u8>,
    pub total_supply: U,
    pub owner_did: IdentityId,
    pub granularity: u128,
    pub decimals: u16,
}

decl_storage! {
    trait Store for Module<T: Trait> as Asset {
        // The DID of the fee collector
        FeeCollector get(fee_collector) config(): T::AccountId;
        // details of the token corresponding to the token ticker
        pub Tokens get(token_details): map Vec<u8> => SecurityToken<T::TokenBalance>;
        // (ticker, did) -> balance
        pub BalanceOf get(balance_of): map (Vec<u8>, IdentityId) => T::TokenBalance;
        // (ticker, sender, spender) -> allowance amount
        Allowance get(allowance): map (Vec<u8>, IdentityId, IdentityId) => T::TokenBalance;
        // cost in base currency to create a token
        AssetCreationFee get(asset_creation_fee) config(): T::Balance;
        // Checkpoints created per token
        pub TotalCheckpoints get(total_checkpoints_of): map (Vec<u8>) => u64;
        // Total supply of the token at the checkpoint
        pub CheckpointTotalSupply get(total_supply_at): map (Vec<u8>, u64) => T::TokenBalance;
        // Balance of a DID at a checkpoint; (ticker, DID, checkpoint ID)
        CheckpointBalance get(balance_at_checkpoint): map (Vec<u8>, IdentityId, u64) => T::TokenBalance;
        // Last checkpoint updated for a DID's balance; (ticker, DID) -> List of checkpoints where user balance changed
        UserCheckpoints get(user_checkpoints): map (Vec<u8>, IdentityId) => Vec<u64>;
        // The documents attached to the tokens
        // (ticker, document name) -> (URI, document hash)
        Documents get(documents): map (Vec<u8>, Vec<u8>) => (Vec<u8>, Vec<u8>, T::Moment);
    }
}

// public interface for this runtime module
decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {
        // initialize the default event for this module
        fn deposit_event() = default;

        // multiple tokens in one go
        pub fn batch_create_token(origin, did: IdentityId, names: Vec<Vec<u8>>, tickers: Vec<Vec<u8>>, total_supply_values: Vec<T::TokenBalance>, divisible_values: Vec<bool>) -> Result {
            let sender = ensure_signed(origin)?;
            let sender_key = Key::try_from( sender.encode())?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, &sender_key), "sender must be a signing key for DID");

            // Ensure we get a complete set of parameters for every token
            ensure!((names.len() == tickers.len()) == (total_supply_values.len() == divisible_values.len()), "Inconsistent token param vector lengths");

            // bytes_to_upper() all tickers
            let mut tickers = tickers;
            tickers.iter_mut().for_each(|ticker| {
                *ticker = utils::bytes_to_upper(ticker.as_slice());
            });

            // A helper vec for duplicate ticker detection
            let mut seen_tickers = Vec::new();

            let n_tokens = names.len();

            // Perform per-token checks beforehand
            for i in 0..n_tokens {
                // checking max size for name and ticker
                // byte arrays (vecs) with no max size should be avoided
                ensure!(names[i].len() <= 64, "token name cannot exceed 64 bytes");
                ensure!(tickers[i].len() <= 32, "token ticker cannot exceed 32 bytes");

                ensure!(!seen_tickers.contains(&tickers[i]), "Duplicate tickers in token batch");
                seen_tickers.push(tickers[i].clone());

                let granularity = if !divisible_values[i] { (10 as u128).pow(18) } else { 1_u128 };
                ensure!(<T as utils::Trait>::as_u128(total_supply_values[i]) % granularity == (0 as u128), "Invalid Total supply");

                // Ensure the uniqueness of the ticker
                ensure!(!<Tokens<T>>::exists(tickers[i].clone()), "Ticker is already issued");
            }
            // TODO: Fix fee withdrawal
            // Withdraw n_tokens * Self::asset_creation_fee() from sender DID
            // let validators = <session::Module<T>>::validators();
            // let fee = Self::asset_creation_fee().checked_mul(&<FeeOf<T> as As<usize>>::sa(n_tokens)).ok_or("asset_creation_fee() * n_tokens overflows")?;
            // let validator_len;
            // if validators.len() < 1 {
            //     validator_len = <FeeOf<T> as As<usize>>::sa(1);
            // } else {
            //     validator_len = <FeeOf<T> as As<usize>>::sa(validators.len());
            // }
            // let proportional_fee = fee / validator_len;
            // let proportional_fee_in_balance = <T::CurrencyToBalance as Convert<FeeOf<T>, T::Balance>>::convert(proportional_fee);
            // for v in &validators {
            //     <balances::Module<T> as Currency<_>>::transfer(&sender, v, proportional_fee_in_balance)?;
            // }
            // let remainder_fee = fee - (proportional_fee * validator_len);
            // let remainder_fee_balance = <T::CurrencyToBalance as Convert<FeeOf<T>, T::Balance>>::convert(proportional_fee);
            // <identity::DidRecords<T>>::mutate(did, |record| -> Result {
            //     record.balance = record.balance.checked_sub(&remainder_fee_balance).ok_or("Could not charge for token issuance")?;
            //     Ok(())
            // })?;

            // Perform per-ticker issuance
            for i in 0..n_tokens {
                let granularity = if !divisible_values[i] { (10 as u128).pow(18) } else { 1_u128 };
                let token = SecurityToken {
                    name: names[i].clone(),
                    total_supply: total_supply_values[i],
                    owner_did: did,
                    granularity: granularity,
                    decimals: 18
                };

                let reg_entry = RegistryEntry { token_type: TokenType::AssetToken as u32, owner_did: did };

                <registry::Module<T>>::put(&tickers[i], &reg_entry)?;

                <Tokens<T>>::insert(&tickers[i], token);
                <BalanceOf<T>>::insert((tickers[i].clone(), did), total_supply_values[i]);
                Self::deposit_event(RawEvent::IssuedToken(tickers[i].clone(), total_supply_values[i], did, granularity, 18));
                sr_primitives::print("Batch token initialized");
            }

            Ok(())
        }

        // initializes a new token
        // takes a name, ticker, total supply for the token
        // makes the initiating account the owner of the token
        // the balance of the owner is set to total supply
        pub fn create_token(origin, did: IdentityId, name: Vec<u8>, _ticker: Vec<u8>, total_supply: T::TokenBalance, divisible: bool) -> Result {
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            let sender = ensure_signed(origin)?;
            let sender_key = Key::try_from(sender.encode())?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, &sender_key), "sender must be a signing key for DID");

            // checking max size for name and ticker
            // byte arrays (vecs) with no max size should be avoided
            ensure!(name.len() <= 64, "token name cannot exceed 64 bytes");
            ensure!(ticker.len() <= 32, "token ticker cannot exceed 32 bytes");

            let granularity = if !divisible { (10 as u128).pow(18) } else { 1_u128 };
            ensure!(<T as utils::Trait>::as_u128(total_supply) % granularity == (0 as u128), "Invalid Total supply");

            ensure!(<registry::Module<T>>::get(&ticker).is_none(), "Ticker is already taken");

            // Alternative way to take a fee - fee is proportionaly paid to the validators and dust is burned
            let validators = <session::Module<T>>::validators();
            let fee = Self::asset_creation_fee();
            let validator_len:T::Balance;
            if validators.len() < 1 {
                validator_len = T::Balance::from(1 as u32);
            } else {
                validator_len = T::Balance::from(validators.len() as u32);
            }
            let proportional_fee = fee / validator_len;
            for v in validators {
                <balances::Module<T> as Currency<_>>::transfer(
                    &sender,
                    &<T as utils::Trait>::validator_id_to_account_id(v),
                    proportional_fee
                )?;
            }
            let remainder_fee = fee - (proportional_fee * validator_len);
            let _withdraw_result = <balances::Module<T>>::withdraw(&sender, remainder_fee, WithdrawReason::Fee, ExistenceRequirement::KeepAlive)?;

            let token = SecurityToken {
                name,
                total_supply,
                owner_did: did,
                granularity: granularity,
                decimals: 18
            };

            let reg_entry = RegistryEntry { token_type: TokenType::AssetToken as u32, owner_did: did };

            <registry::Module<T>>::put(&ticker, &reg_entry)?;

            <Tokens<T>>::insert(&ticker, token);
            <BalanceOf<T>>::insert((ticker.clone(), did), total_supply);
            Self::deposit_event(RawEvent::IssuedToken(ticker, total_supply, did, granularity, 18));
            sr_primitives::print("Initialized!!!");

            Ok(())
        }

        // transfer tokens from one account to another
        // origin is assumed as sender
        pub fn transfer(_origin, did: IdentityId, _ticker: Vec<u8>, to_did: IdentityId, value: T::TokenBalance) -> Result {
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            let sender = ensure_signed(_origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, & Key::try_from(sender.encode())?), "sender must be a signing key for DID");

            ensure!(Self::_is_valid_transfer(&ticker, Some(did), Some(to_did), value)? == ERC1400_TRANSFER_SUCCESS, "Transfer restrictions failed");

            Self::_transfer(&ticker, did, to_did, value)
        }

        /// Forces a transfer between two accounts. Can only be called by token owner
        pub fn controller_transfer(_origin, did: IdentityId, _ticker: Vec<u8>, from_did: IdentityId, to_did: IdentityId, value: T::TokenBalance, data: Vec<u8>, operator_data: Vec<u8>) -> Result {
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            let sender = ensure_signed(_origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, & Key::try_from( sender.encode())?), "sender must be a signing key for DID");

            ensure!(Self::is_owner(&ticker, did), "user is not authorized");

            Self::_transfer(&ticker, from_did, to_did, value.clone())?;

            Self::deposit_event(RawEvent::ControllerTransfer(ticker, did, from_did, to_did, value, data, operator_data));

            Ok(())
        }

        // approve token transfer from one account to another
        // once this is done, transfer_from can be called with corresponding values
        fn approve(_origin, did: IdentityId, _ticker: Vec<u8>, spender_did: IdentityId, value: T::TokenBalance) -> Result {
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            let sender = ensure_signed(_origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, & Key::try_from( sender.encode())?), "sender must be a signing key for DID");

            ensure!(<BalanceOf<T>>::exists((ticker.clone(), did)), "Account does not own this token");

            let allowance = Self::allowance((ticker.clone(), did, spender_did));
            let updated_allowance = allowance.checked_add(&value).ok_or("overflow in calculating allowance")?;
            <Allowance<T>>::insert((ticker.clone(), did, spender_did), updated_allowance);

            Self::deposit_event(RawEvent::Approval(ticker, did, spender_did, value));

            Ok(())
        }

        // implemented in the open-zeppelin way - increase/decrease allownace
        // if approved, transfer from an account to another account without owner's signature
        pub fn transfer_from(_origin, did: IdentityId, _ticker: Vec<u8>, from_did: IdentityId, to_did: IdentityId, value: T::TokenBalance) -> Result {
            let spender = ensure_signed(_origin)?;

            // Check that spender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, & Key::try_from( spender.encode())?), "sender must be a signing key for DID");

            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            let ticker_from_did_did = (ticker.clone(), from_did, did);
            ensure!(<Allowance<T>>::exists(&ticker_from_did_did), "Allowance does not exist");
            let allowance = Self::allowance(&ticker_from_did_did);
            ensure!(allowance >= value, "Not enough allowance");

            // using checked_sub (safe math) to avoid overflow
            let updated_allowance = allowance.checked_sub(&value).ok_or("overflow in calculating allowance")?;

            ensure!(Self::_is_valid_transfer(&ticker, Some(from_did), Some(to_did), value)? == ERC1400_TRANSFER_SUCCESS, "Transfer restrictions failed");

            Self::_transfer(&ticker, from_did, to_did, value)?;

            // Change allowance afterwards
            <Allowance<T>>::insert(&ticker_from_did_did, updated_allowance);

            Self::deposit_event(RawEvent::Approval(ticker, from_did, did, value));
            Ok(())
        }

        // called by issuer to create checkpoints
        pub fn create_checkpoint(_origin, did: IdentityId, _ticker: Vec<u8>) -> Result {
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            let sender = ensure_signed(_origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, & Key::try_from( sender.encode())?), "sender must be a signing key for DID");

            ensure!(Self::is_owner(&ticker, did), "user is not authorized");
            Self::_create_checkpoint(&ticker)
        }

        pub fn issue(origin, did: IdentityId, ticker: Vec<u8>, to_did: IdentityId, value: T::TokenBalance, _data: Vec<u8>) -> Result {
            let upper_ticker = utils::bytes_to_upper(&ticker);
            let sender = ensure_signed(origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, & Key::try_from( sender.encode())?), "sender must be a signing key for DID");

            ensure!(Self::is_owner(&upper_ticker, did), "user is not authorized");
            Self::_mint(&upper_ticker, to_did, value)
        }

        // Mint a token to multiple investors
        pub fn batch_issue(origin, did: IdentityId, ticker: Vec<u8>, investor_dids: Vec<IdentityId>, values: Vec<T::TokenBalance>) -> Result {
            let sender = ensure_signed(origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, &Key::try_from( sender.encode())?), "sender must be a signing key for DID");

            ensure!(investor_dids.len() == values.len(), "Investor/amount list length inconsistent");

            ensure!(Self::is_owner(&ticker, did), "user is not authorized");


            // A helper vec for calculated new investor balances
            let mut updated_balances = Vec::with_capacity(investor_dids.len());

            // A helper vec for calculated new investor balances
            let mut current_balances = Vec::with_capacity(investor_dids.len());

            // Get current token details for supply update
            let mut token = Self::token_details(ticker.clone());

            // A round of per-investor checks
            for i in 0..investor_dids.len() {
                ensure!(
                    Self::check_granularity(&ticker, values[i]),
                    "Invalid granularity"
                );

                current_balances.push(Self::balance_of((ticker.clone(), investor_dids[i].clone())));
                updated_balances.push(current_balances[i]
                    .checked_add(&values[i])
                    .ok_or("overflow in calculating balance")?);

                // verify transfer check
                ensure!(Self::_is_valid_transfer(&ticker, None, Some(investor_dids[i]), values[i])? == ERC1400_TRANSFER_SUCCESS, "Transfer restrictions failed");

                // New total supply must be valid
                token.total_supply = token
                    .total_supply
                    .checked_add(&values[i])
                    .ok_or("overflow in calculating balance")?;
            }

            // After checks are ensured introduce side effects
            for i in 0..investor_dids.len() {
                Self::_update_checkpoint(&ticker, investor_dids[i], current_balances[i]);

                <BalanceOf<T>>::insert((ticker.clone(), investor_dids[i]), updated_balances[i]);

                Self::deposit_event(RawEvent::Issued(ticker.clone(), investor_dids[i], values[i]));
            }
            <Tokens<T>>::insert(ticker.clone(), token);

            Ok(())
        }

        pub fn redeem(_origin, did: IdentityId, _ticker: Vec<u8>, value: T::TokenBalance, _data: Vec<u8>) -> Result {
            let upper_ticker = utils::bytes_to_upper(_ticker.as_slice());
            let sender = ensure_signed(_origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, &Key::try_from(sender.encode())?), "sender must be a signing key for DID");

            // Granularity check
            ensure!(
                Self::check_granularity(&upper_ticker, value),
                "Invalid granularity"
                );
            let ticker_did = (upper_ticker.clone(), did);
            ensure!(<BalanceOf<T>>::exists(&ticker_did), "Account does not own this token");
            let burner_balance = Self::balance_of(&ticker_did);
            ensure!(burner_balance >= value, "Not enough balance.");

            // Reduce sender's balance
            let updated_burner_balance = burner_balance
                .checked_sub(&value)
                .ok_or("overflow in calculating balance")?;

            // verify transfer check
            ensure!(Self::_is_valid_transfer(&upper_ticker, Some(did), None, value)? == ERC1400_TRANSFER_SUCCESS, "Transfer restrictions failed");

            //Decrease total supply
            let mut token = Self::token_details(&upper_ticker);
            token.total_supply = token.total_supply.checked_sub(&value).ok_or("overflow in calculating balance")?;

            Self::_update_checkpoint(&upper_ticker, did, burner_balance);

            <BalanceOf<T>>::insert((upper_ticker.clone(), did), updated_burner_balance);
            <Tokens<T>>::insert(&upper_ticker, token);

            Self::deposit_event(RawEvent::Redeemed(upper_ticker, did, value));

            Ok(())

        }

        pub fn redeem_from(_origin, did: IdentityId, _ticker: Vec<u8>, from_did: IdentityId, value: T::TokenBalance, _data: Vec<u8>) -> Result {
            let upper_ticker = utils::bytes_to_upper(_ticker.as_slice());
            let sender = ensure_signed(_origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, &Key::try_from(sender.encode())?), "sender must be a signing key for DID");

            // Granularity check
            ensure!(
                Self::check_granularity(&upper_ticker, value),
                "Invalid granularity"
                );
            let ticker_did = (upper_ticker.clone(), did);
            ensure!(<BalanceOf<T>>::exists(&ticker_did), "Account does not own this token");
            let burner_balance = Self::balance_of(&ticker_did);
            ensure!(burner_balance >= value, "Not enough balance.");

            // Reduce sender's balance
            let updated_burner_balance = burner_balance
                .checked_sub(&value)
                .ok_or("overflow in calculating balance")?;

            let ticker_from_did_did = (upper_ticker.clone(), from_did, did);
            ensure!(<Allowance<T>>::exists(&ticker_from_did_did), "Allowance does not exist");
            let allowance = Self::allowance(&ticker_from_did_did);
            ensure!(allowance >= value, "Not enough allowance");

            ensure!(Self::_is_valid_transfer( &upper_ticker, Some(from_did), None, value)? == ERC1400_TRANSFER_SUCCESS, "Transfer restrictions failed");

            let updated_allowance = allowance.checked_sub(&value).ok_or("overflow in calculating allowance")?;

            //Decrease total suply
            let mut token = Self::token_details(&upper_ticker);
            token.total_supply = token.total_supply.checked_sub(&value).ok_or("overflow in calculating balance")?;

            Self::_update_checkpoint(&upper_ticker, did, burner_balance);

            <Allowance<T>>::insert(&ticker_from_did_did, updated_allowance);
            <BalanceOf<T>>::insert(&ticker_did, updated_burner_balance);
            <Tokens<T>>::insert(&upper_ticker, token);

            Self::deposit_event(RawEvent::Redeemed(upper_ticker.clone(), did, value));
            Self::deposit_event(RawEvent::Approval(upper_ticker, from_did, did, value));

            Ok(())
        }

        /// Forces a redemption of an account's tokens. Can only be called by token owner
        pub fn controller_redeem(origin, did: IdentityId, ticker: Vec<u8>, token_holder_did: IdentityId, value: T::TokenBalance, data: Vec<u8>, operator_data: Vec<u8>) -> Result {
            let ticker = utils::bytes_to_upper(ticker.as_slice());
            let sender = ensure_signed(origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, &Key::try_from(sender.encode())?), "sender must be a signing key for DID");
            ensure!(Self::is_owner(&ticker, did), "user is not token owner");

            // Granularity check
            ensure!(
                Self::check_granularity(&ticker, value),
                "Invalid granularity"
                );
            let ticker_token_holder_did = (ticker.clone(), token_holder_did);
            ensure!(<BalanceOf<T>>::exists( &ticker_token_holder_did), "Account does not own this token");
            let burner_balance = Self::balance_of(&ticker_token_holder_did);
            ensure!(burner_balance >= value, "Not enough balance.");

            // Reduce sender's balance
            let updated_burner_balance = burner_balance
                .checked_sub(&value)
                .ok_or("overflow in calculating balance")?;

            //Decrease total suply
            let mut token = Self::token_details(&ticker);
            token.total_supply = token.total_supply.checked_sub(&value).ok_or("overflow in calculating balance")?;

            Self::_update_checkpoint(&ticker, token_holder_did, burner_balance);

            <BalanceOf<T>>::insert(&ticker_token_holder_did, updated_burner_balance);
            <Tokens<T>>::insert(&ticker, token);

            Self::deposit_event(RawEvent::ControllerRedemption(ticker, did, token_holder_did, value, data, operator_data));

            Ok(())
        }


        pub fn change_granularity(origin, did: IdentityId, ticker: Vec<u8>, granularity: u128) -> Result {
            let ticker = utils::bytes_to_upper(ticker.as_slice());
            let sender = ensure_signed(origin)?;

            // Check that sender is allowed to act on behalf of `did`
            ensure!(<identity::Module<T>>::is_signing_key(did, &Key::try_from(sender.encode())?), "sender must be a signing key for DID");

            ensure!(Self::is_owner(&ticker, did), "user is not authorized");
            ensure!(granularity != 0_u128, "Invalid granularity");
            // Read the token details
            let mut token = Self::token_details(&ticker);
            //Increase total suply
            token.granularity = granularity;
            <Tokens<T>>::insert(&ticker, token);
            Self::deposit_event(RawEvent::GranularityChanged(ticker, granularity));
            Ok(())
        }

        /// Checks whether a transaction with given parameters can take place
        pub fn can_transfer(_origin, ticker: Vec<u8>, from_did: IdentityId, to_did: IdentityId, value: T::TokenBalance, data: Vec<u8>) {
            match Self::_is_valid_transfer(&ticker, Some(from_did), Some(to_did), value) {
                Ok(code) =>
                {
                    Self::deposit_event(RawEvent::CanTransfer(ticker, from_did, to_did, value, data, code as u32));
                },
                Err(msg) => {
                    // We emit a generic error with the event whenever there's an internal issue - i.e. captured
                    // in a string error and not using the status codes
                    sr_primitives::print(msg);
                    Self::deposit_event(RawEvent::CanTransfer(ticker, from_did, to_did, value, data, ERC1400_TRANSFER_FAILURE as u32));
                }
            }
        }

    /// An ERC1594 transfer with data
    pub fn transfer_with_data(origin, did: IdentityId, ticker: Vec<u8>, to_did: IdentityId, value: T::TokenBalance, data: Vec<u8>) -> Result {
        Self::transfer(origin, did, ticker.clone(), to_did, value)?;
        Self::deposit_event(RawEvent::TransferWithData(ticker, did, to_did, value, data));
        Ok(())
    }

    /// An ERC1594 transfer_from with data
    pub fn transfer_from_with_data(origin, did: IdentityId, ticker: Vec<u8>, from_did: IdentityId, to_did: IdentityId, value: T::TokenBalance, data: Vec<u8>) -> Result {
        Self::transfer_from(origin, did, ticker.clone(), from_did,  to_did, value)?;
        Self::deposit_event(RawEvent::TransferWithData(ticker, from_did, to_did, value, data));
        Ok(())
    }


    pub fn is_issuable(_origin, ticker: Vec<u8>) {
        Self::deposit_event(RawEvent::IsIssuable(ticker, true));
    }

    pub fn get_document(_origin, ticker: Vec<u8>, name: Vec<u8>) -> Result {
        let record = <Documents<T>>::get((ticker.clone(), name.clone()));
        Self::deposit_event(RawEvent::GetDocument(ticker, name, record.0, record.1, record.2));
        Ok(())
    }

    pub fn set_document(origin, did: IdentityId, ticker: Vec<u8>, name: Vec<u8>, uri: Vec<u8>, document_hash: Vec<u8>) -> Result {
        let ticker = utils::bytes_to_upper(ticker.as_slice());
        let sender = ensure_signed(origin)?;

        // Check that sender is allowed to act on behalf of `did`
        ensure!(<identity::Module<T>>::is_signing_key(did, &Key::try_from(sender.encode())?), "sender must be a signing key for DID");
        ensure!(Self::is_owner(&ticker, did), "user is not authorized");

        <Documents<T>>::insert((ticker, name), (uri, document_hash, <timestamp::Module<T>>::get()));
        Ok(())
    }

    pub fn remove_document(origin, did: IdentityId, ticker: Vec<u8>, name: Vec<u8>) -> Result {
        let ticker = utils::bytes_to_upper(ticker.as_slice());
        let sender = ensure_signed(origin)?;

        // Check that sender is allowed to act on behalf of `did`
        ensure!(<identity::Module<T>>::is_signing_key(did, &Key::try_from(sender.encode())?), "sender must be a signing key for DID");
        ensure!(Self::is_owner(&ticker, did), "user is not authorized");

        <Documents<T>>::remove((ticker, name));
        Ok(())
    }
}
}

decl_event! {
    pub enum Event<T>
        where
        Balance = <T as utils::Trait>::TokenBalance,
        Moment = <T as timestamp::Trait>::Moment,
        {
            // event for transfer of tokens
            // ticker, from DID, to DID, value
            Transfer(Vec<u8>, IdentityId, IdentityId, Balance),
            // event when an approval is made
            // ticker, owner DID, spender DID, value
            Approval(Vec<u8>, IdentityId, IdentityId, Balance),

            // ticker, beneficiary DID, value
            Issued(Vec<u8>, IdentityId, Balance),

            // ticker, DID, value
            Redeemed(Vec<u8>, IdentityId, Balance),
            // event for forced transfer of tokens
            // ticker, controller DID, from DID, to DID, value, data, operator data
            ControllerTransfer(Vec<u8>, IdentityId, IdentityId, IdentityId, Balance, Vec<u8>, Vec<u8>),

            // event for when a forced redemption takes place
            // ticker, controller DID, token holder DID, value, data, operator data
            ControllerRedemption(Vec<u8>, IdentityId, IdentityId, Balance, Vec<u8>, Vec<u8>),

            // Event for creation of the asset
            // ticker, total supply, owner DID, decimal
            IssuedToken(Vec<u8>, Balance, IdentityId, u128, u16),
            // Event for change granularity
            // ticker, granularity
            GranularityChanged(Vec<u8>, u128),

            // can_transfer() output
            // ticker, from_did, to_did, value, data, ERC1066 status
            // 0 - OK
            // 1,2... - Error, meanings TBD
            CanTransfer(Vec<u8>, IdentityId, IdentityId, Balance, Vec<u8>, u32),

            // An additional event to Transfer; emitted when transfer_with_data is called; similar to
            // Transfer with data added at the end.
            // ticker, from DID, to DID, value, data
            TransferWithData(Vec<u8>, IdentityId, IdentityId, Balance, Vec<u8>),

            // is_issuable() output
            // ticker, return value (true if issuable)
            IsIssuable(Vec<u8>, bool),

            // get_document() output
            // ticker, name, uri, hash, last modification date
            GetDocument(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Moment),
        }
}

pub trait AssetTrait<V> {
    fn total_supply(ticker: &[u8]) -> V;
    fn balance(ticker: &[u8], did: IdentityId) -> V;
    fn _mint_from_sto(ticker: &[u8], sender_did: IdentityId, tokens_purchased: V) -> Result;
    fn is_owner(ticker: &Vec<u8>, did: IdentityId) -> bool;
    fn get_balance_at(ticker: &Vec<u8>, did: IdentityId, at: u32) -> V;
}

impl<T: Trait> AssetTrait<T::TokenBalance> for Module<T> {
    fn _mint_from_sto(
        ticker: &[u8],
        sender: IdentityId,
        tokens_purchased: T::TokenBalance,
    ) -> Result {
        let upper_ticker = utils::bytes_to_upper(ticker);
        Self::_mint(&upper_ticker, sender, tokens_purchased)
    }

    fn is_owner(ticker: &Vec<u8>, did: IdentityId) -> bool {
        Self::_is_owner(ticker, did)
    }

    /// Get the asset `id` balance of `who`.
    fn balance(ticker: &[u8], who: IdentityId) -> T::TokenBalance {
        let upper_ticker = utils::bytes_to_upper(ticker);
        return Self::balance_of((upper_ticker, who));
    }

    // Get the total supply of an asset `id`
    fn total_supply(ticker: &[u8]) -> T::TokenBalance {
        let upper_ticker = utils::bytes_to_upper(ticker);
        return Self::token_details(upper_ticker).total_supply;
    }

    fn get_balance_at(ticker: &Vec<u8>, did: IdentityId, at: u32) -> T::TokenBalance {
        let upper_ticker = utils::bytes_to_upper(ticker);
        return Self::get_balance_at(&upper_ticker, did, at);
    }
}

/// All functions in the decl_module macro become part of the public interface of the module
/// If they are there, they are accessible via extrinsics calls whether they are public or not
/// However, in the impl module section (this, below) the functions can be public and private
/// Private functions are internal to this module e.g.: _transfer
/// Public functions can be called from other modules e.g.: lock and unlock (being called from the tcr module)
/// All functions in the impl module section are not part of public interface because they are not part of the Call enum
impl<T: Trait> Module<T> {
    // Public immutables
    pub fn _is_owner(ticker: &Vec<u8>, did: IdentityId) -> bool {
        let token = Self::token_details(ticker);
        token.owner_did == did
    }

    /// Get the asset `id` balance of `who`.
    pub fn balance(ticker: &Vec<u8>, did: IdentityId) -> T::TokenBalance {
        let upper_ticker = utils::bytes_to_upper(ticker);
        Self::balance_of((upper_ticker, did))
    }

    // Get the total supply of an asset `id`
    pub fn total_supply(ticker: &[u8]) -> T::TokenBalance {
        let upper_ticker = utils::bytes_to_upper(ticker);
        Self::token_details(upper_ticker).total_supply
    }

    pub fn get_balance_at(ticker: &Vec<u8>, did: IdentityId, at: u64) -> T::TokenBalance {
        let upper_ticker = utils::bytes_to_upper(ticker);
        let ticker_did = (upper_ticker.clone(), did);
        if !<TotalCheckpoints>::exists(upper_ticker.clone()) ||
            at == 0 || //checkpoints start from 1
            at > Self::total_checkpoints_of(&upper_ticker)
        {
            // No checkpoints data exist
            return Self::balance_of(&ticker_did);
        }

        if <UserCheckpoints>::exists(&ticker_did) {
            let user_checkpoints = Self::user_checkpoints(&ticker_did);
            if at > *user_checkpoints.last().unwrap_or(&0) {
                // Using unwrap_or to be defensive.
                // or part should never be triggered due to the check on 2 lines above
                // User has not transacted after checkpoint creation.
                // This means their current balance = their balance at that cp.
                return Self::balance_of(&ticker_did);
            }
            // Uses the first checkpoint that was created after target checpoint
            // and the user has data for that checkpoint
            return Self::balance_at_checkpoint((
                upper_ticker.clone(),
                did,
                Self::find_ceiling(&user_checkpoints, at),
            ));
        }
        // User has no checkpoint data.
        // This means that user's balance has not changed since first checkpoint was created.
        // Maybe the user never held any balance.
        return Self::balance_of(&ticker_did);
    }

    fn find_ceiling(arr: &Vec<u64>, key: u64) -> u64 {
        // This function assumes that key <= last element of the array,
        // the array consists of unique sorted elements,
        // array len > 0
        let mut end = arr.len();
        let mut start = 0;
        let mut mid = (start + end) / 2;

        while mid != 0 && end >= start {
            // Due to our assumptions, we can even remove end >= start condition from here
            if key > arr[mid - 1] && key <= arr[mid] {
                // This condition and the fact that key <= last element of the array mean that
                // start should never become greater than end.
                return arr[mid];
            } else if key > arr[mid] {
                start = mid + 1;
            } else {
                end = mid;
            }
            mid = (start + end) / 2;
        }

        // This should only be reached when mid becomes 0.
        return arr[0];
    }

    fn _is_valid_transfer(
        ticker: &Vec<u8>,
        from_did: Option<IdentityId>,
        to_did: Option<IdentityId>,
        value: T::TokenBalance,
    ) -> StdResult<u8, &'static str> {
        let general_status_code =
            <general_tm::Module<T>>::verify_restriction(ticker, from_did, to_did, value)?;
        Ok(if general_status_code != ERC1400_TRANSFER_SUCCESS {
            general_status_code
        } else {
            <percentage_tm::Module<T>>::verify_restriction(ticker, from_did, to_did, value)?
        })
    }

    // the SimpleToken standard transfer function
    // internal
    fn _transfer(
        ticker: &Vec<u8>,
        from_did: IdentityId,
        to_did: IdentityId,
        value: T::TokenBalance,
    ) -> Result {
        // Granularity check
        ensure!(
            Self::check_granularity(ticker, value),
            "Invalid granularity"
        );
        let ticket_from_did = (ticker.clone(), from_did);
        ensure!(
            <BalanceOf<T>>::exists(&ticket_from_did),
            "Account does not own this token"
        );
        let sender_balance = Self::balance_of(&ticket_from_did);
        ensure!(sender_balance >= value, "Not enough balance.");

        let updated_from_balance = sender_balance
            .checked_sub(&value)
            .ok_or("overflow in calculating balance")?;
        let ticket_to_did = (ticker.clone(), to_did);
        let receiver_balance = Self::balance_of(&ticket_to_did);
        let updated_to_balance = receiver_balance
            .checked_add(&value)
            .ok_or("overflow in calculating balance")?;

        Self::_update_checkpoint(ticker, from_did, sender_balance);
        Self::_update_checkpoint(ticker, to_did, receiver_balance);
        // reduce sender's balance
        <BalanceOf<T>>::insert(ticket_from_did, updated_from_balance);

        // increase receiver's balance
        <BalanceOf<T>>::insert(ticket_to_did, updated_to_balance);

        Self::deposit_event(RawEvent::Transfer(ticker.clone(), from_did, to_did, value));
        Ok(())
    }

    pub fn _create_checkpoint(ticker: &Vec<u8>) -> Result {
        if <TotalCheckpoints>::exists(ticker) {
            let mut checkpoint_count = Self::total_checkpoints_of(ticker);
            checkpoint_count = checkpoint_count
                .checked_add(1)
                .ok_or("overflow in adding checkpoint")?;
            <TotalCheckpoints>::insert(ticker, checkpoint_count);
            <CheckpointTotalSupply<T>>::insert(
                (ticker.clone(), checkpoint_count),
                Self::token_details(ticker).total_supply,
            );
        } else {
            <TotalCheckpoints>::insert(ticker, 1);
            <CheckpointTotalSupply<T>>::insert(
                (ticker.clone(), 1),
                Self::token_details(ticker).total_supply,
            );
        }
        Ok(())
    }

    fn _update_checkpoint(ticker: &Vec<u8>, user_did: IdentityId, user_balance: T::TokenBalance) {
        if <TotalCheckpoints>::exists(ticker) {
            let checkpoint_count = Self::total_checkpoints_of(ticker);
            let ticker_user_did_checkpont = (ticker.clone(), user_did, checkpoint_count);
            if !<CheckpointBalance<T>>::exists(&ticker_user_did_checkpont) {
                <CheckpointBalance<T>>::insert(&ticker_user_did_checkpont, user_balance);
                <UserCheckpoints>::mutate((ticker.clone(), user_did), |user_checkpoints| {
                    user_checkpoints.push(checkpoint_count);
                });
            }
        }
    }

    fn is_owner(ticker: &Vec<u8>, did: IdentityId) -> bool {
        Self::_is_owner(ticker, did)
    }

    pub fn _mint(ticker: &Vec<u8>, to_did: IdentityId, value: T::TokenBalance) -> Result {
        // Granularity check
        ensure!(
            Self::check_granularity(ticker, value),
            "Invalid granularity"
        );
        //Increase receiver balance
        let ticker_to_did = (ticker.clone(), to_did);
        let current_to_balance = Self::balance_of(&ticker_to_did);
        let updated_to_balance = current_to_balance
            .checked_add(&value)
            .ok_or("overflow in calculating balance")?;
        // verify transfer check
        ensure!(
            Self::_is_valid_transfer(ticker, None, Some(to_did), value)?
                == ERC1400_TRANSFER_SUCCESS,
            "Transfer restrictions failed"
        );

        // Read the token details
        let mut token = Self::token_details(ticker);
        //Increase total suply
        token.total_supply = token
            .total_supply
            .checked_add(&value)
            .ok_or("overflow in calculating balance")?;

        Self::_update_checkpoint(ticker, to_did, current_to_balance);

        <BalanceOf<T>>::insert(&ticker_to_did, updated_to_balance);
        <Tokens<T>>::insert(ticker, token);

        Self::deposit_event(RawEvent::Issued(ticker.clone(), to_did, value));

        Ok(())
    }

    fn check_granularity(ticker: &Vec<u8>, value: T::TokenBalance) -> bool {
        // Read the token details
        let token = Self::token_details(ticker);
        // Check the granularity
        <T as utils::Trait>::as_u128(value) % token.granularity == (0 as u128)
    }
}

/// tests for this module
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{exemption, identity};
    use primitives::{IdentityId, Key};
    use rand::Rng;

    use chrono::prelude::*;
    use lazy_static::lazy_static;
    use sr_io::with_externalities;
    use sr_primitives::{
        testing::{Header, UintAuthorityId},
        traits::{BlakeTwo256, ConvertInto, IdentityLookup, OpaqueKeys},
        Perbill,
    };
    use srml_support::{assert_ok, impl_outer_origin, parameter_types};
    use std::sync::{Arc, Mutex};
    use substrate_primitives::{Blake2Hasher, H256};

    type SessionIndex = u32;
    type AuthorityId = u64;
    type BlockNumber = u64;

    pub struct TestOnSessionEnding;
    impl session::OnSessionEnding<AuthorityId> for TestOnSessionEnding {
        fn on_session_ending(_: SessionIndex, _: SessionIndex) -> Option<Vec<AuthorityId>> {
            None
        }
    }

    pub struct TestSessionHandler;
    impl session::SessionHandler<AuthorityId> for TestSessionHandler {
        fn on_new_session<Ks: OpaqueKeys>(
            _changed: bool,
            _validators: &[(AuthorityId, Ks)],
            _queued_validators: &[(AuthorityId, Ks)],
        ) {
        }

        fn on_disabled(_validator_index: usize) {}

        fn on_genesis_session<Ks: OpaqueKeys>(_validators: &[(AuthorityId, Ks)]) {}
    }

    impl_outer_origin! {
        pub enum Origin for Test {}
    }

    // For testing the module, we construct most of a mock runtime. This means
    // first constructing a configuration type (`Test`) which `impl`s each of the
    // configuration traits of modules we want to use.
    #[derive(Clone, Eq, PartialEq)]
    pub struct Test;
    parameter_types! {
        pub const Period: BlockNumber = 1;
        pub const Offset: BlockNumber = 0;
        pub const BlockHashCount: u32 = 250;
        pub const MaximumBlockWeight: u32 = 4 * 1024 * 1024;
        pub const MaximumBlockLength: u32 = 4 * 1024 * 1024;
        pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
    }
    impl system::Trait for Test {
        type Origin = Origin;
        type Call = ();
        type Index = u64;
        type BlockNumber = BlockNumber;
        type Hash = H256;
        type Hashing = BlakeTwo256;
        type AccountId = u64;
        // type AccountId = <AnySignature as Verify>::Signer;
        type Lookup = IdentityLookup<u64>;
        type WeightMultiplierUpdate = ();
        type Header = Header;
        type Event = ();
        type BlockHashCount = BlockHashCount;
        type MaximumBlockWeight = MaximumBlockWeight;
        type AvailableBlockRatio = AvailableBlockRatio;
        type MaximumBlockLength = MaximumBlockLength;
        type Version = ();
    }

    parameter_types! {
        pub const DisabledValidatorsThreshold: Perbill = Perbill::from_percent(33);
    }

    impl session::Trait for Test {
        type OnSessionEnding = TestOnSessionEnding;
        type Keys = UintAuthorityId;
        type ShouldEndSession = session::PeriodicSessions<Period, Offset>;
        type SessionHandler = TestSessionHandler;
        type Event = ();
        type ValidatorId = AuthorityId;
        type ValidatorIdOf = ConvertInto;
        type SelectInitialValidators = ();
        type DisabledValidatorsThreshold = DisabledValidatorsThreshold;
    }

    impl session::historical::Trait for Test {
        type FullIdentification = ();
        type FullIdentificationOf = ();
    }

    parameter_types! {
        pub const ExistentialDeposit: u64 = 0;
        pub const TransferFee: u64 = 0;
        pub const CreationFee: u64 = 0;
        pub const TransactionBaseFee: u64 = 0;
        pub const TransactionByteFee: u64 = 0;
    }

    impl balances::Trait for Test {
        type Balance = u128;
        type OnFreeBalanceZero = ();
        type OnNewAccount = ();
        type Event = ();
        type TransactionPayment = ();
        type DustRemoval = ();
        type TransferPayment = ();
        type ExistentialDeposit = ExistentialDeposit;
        type TransferFee = TransferFee;
        type CreationFee = CreationFee;
        type TransactionBaseFee = TransactionBaseFee;
        type TransactionByteFee = TransactionByteFee;
        type WeightToFee = ConvertInto;
        type Identity = identity::Module<Test>;
    }

    impl general_tm::Trait for Test {
        type Event = ();
        type Asset = Module<Test>;
    }
    impl identity::Trait for Test {
        type Event = ();
    }
    impl percentage_tm::Trait for Test {
        type Event = ();
    }

    impl exemption::Trait for Test {
        type Event = ();
        type Asset = Module<Test>;
    }

    parameter_types! {
        pub const MinimumPeriod: u64 = 3;
    }

    impl timestamp::Trait for Test {
        type Moment = u64;
        type OnTimestampSet = ();
        type MinimumPeriod = MinimumPeriod;
    }

    impl utils::Trait for Test {
        type TokenBalance = u128;
        fn as_u128(v: Self::TokenBalance) -> u128 {
            v
        }
        fn as_tb(v: u128) -> Self::TokenBalance {
            v
        }
        fn token_balance_to_balance(v: Self::TokenBalance) -> <Self as balances::Trait>::Balance {
            v
        }
        fn balance_to_token_balance(v: <Self as balances::Trait>::Balance) -> Self::TokenBalance {
            v
        }
        fn validator_id_to_account_id(v: <Self as session::Trait>::ValidatorId) -> Self::AccountId {
            v
        }
    }

    impl registry::Trait for Test {}
    impl Trait for Test {
        type Event = ();
        type Currency = balances::Module<Test>;
    }
    type Asset = Module<Test>;
    type Balances = balances::Module<Test>;
    type Identity = identity::Module<Test>;
    type GeneralTM = general_tm::Module<Test>;

    lazy_static! {
        static ref INVESTOR_MAP_OUTER_LOCK: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
    }

    /// Build a genesis identity instance owned by account No. 1
    fn identity_owned_by_1() -> sr_io::TestExternalities<Blake2Hasher> {
        let mut t = system::GenesisConfig::default()
            .build_storage::<Test>()
            .unwrap();
        identity::GenesisConfig::<Test> {
            owner: 1,
            did_creation_fee: 250,
        }
        .assimilate_storage(&mut t)
        .unwrap();
        sr_io::TestExternalities::new(t)
    }

    #[test]
    fn issuers_can_create_tokens() {
        with_externalities(&mut identity_owned_by_1(), || {
            let owner_acc = 1;
            let _owner_key = Key::try_from(owner_acc.encode()).unwrap();
            let owner_did = IdentityId::from(owner_acc as u128);

            // Raise the owner's base currency balance
            Balances::make_free_balance_be(&owner_acc, 1_000_000);
            Identity::register_did(Origin::signed(owner_acc), owner_did, vec![])
                .expect("Could not create owner_did");

            // Expected token entry
            let token = SecurityToken {
                name: vec![0x01],
                owner_did: owner_did,
                total_supply: 1_000_000,
                granularity: 1,
                decimals: 18,
            };

            Identity::fund_poly(Origin::signed(owner_acc), owner_did, 500_000)
                .expect("Could not add funds to DID");

            // Issuance is successful
            assert_ok!(Asset::create_token(
                Origin::signed(owner_acc),
                owner_did,
                token.name.clone(),
                token.name.clone(),
                token.total_supply,
                true
            ));

            // A correct entry is added
            assert_eq!(Asset::token_details(token.name.clone()), token);
        });
    }

    /// # TODO
    /// It should be re-enable once issuer claim is re-enabled.
    #[test]
    #[ignore]
    fn non_issuers_cant_create_tokens() {
        with_externalities(&mut identity_owned_by_1(), || {
            let owner_did = IdentityId::from(1u128);
            let owner_acc = 1;

            // Expected token entry
            let token = SecurityToken {
                name: vec![0x01],
                owner_did: owner_did,
                total_supply: 1_000_000,
                granularity: 1,
                decimals: 18,
            };

            let wrong_acc = owner_acc + 1;

            Balances::make_free_balance_be(&wrong_acc, 1_000_000);

            let wrong_did = IdentityId::try_from("did:poly:wrong");
            assert!(wrong_did.is_err());

            // Entry is not added
            assert_ne!(Asset::token_details(token.name.clone()), token);
        });
    }

    #[test]
    fn valid_transfers_pass() {
        with_externalities(&mut identity_owned_by_1(), || {
            let now = Utc::now();
            <timestamp::Module<Test>>::set_timestamp(now.timestamp() as u64);

            let owner_acc = 1;
            let owner_did = IdentityId::from(1u128);

            // Expected token entry
            let token = SecurityToken {
                name: vec![0x01],
                owner_did: owner_did,
                total_supply: 1_000_000,
                granularity: 1,
                decimals: 18,
            };

            Balances::make_free_balance_be(&owner_acc, 1_000_000);
            Identity::register_did(Origin::signed(owner_acc), owner_did, vec![])
                .expect("Could not create owner_did");

            let alice_acc = 2;
            let alice_did = IdentityId::from(2u128);

            Balances::make_free_balance_be(&alice_acc, 1_000_000);
            Identity::register_did(Origin::signed(alice_acc), alice_did, vec![])
                .expect("Could not create alice_did");
            let bob_acc = 3;
            let bob_did = IdentityId::from(3u128);

            Balances::make_free_balance_be(&bob_acc, 1_000_000);
            Identity::register_did(Origin::signed(bob_acc), bob_did, vec![])
                .expect("Could not create bob_did");
            Identity::fund_poly(Origin::signed(owner_acc), owner_did, 500_000)
                .expect("Could not add funds to DID");

            // Issuance is successful
            assert_ok!(Asset::create_token(
                Origin::signed(owner_acc),
                owner_did,
                token.name.clone(),
                token.name.clone(),
                token.total_supply,
                true
            ));

            // A correct entry is added
            assert_eq!(Asset::token_details(token.name.clone()), token);

            let asset_rule = general_tm::AssetRule {
                sender_rules: vec![],
                receiver_rules: vec![],
            };

            // Allow all transfers
            assert_ok!(GeneralTM::add_active_rule(
                Origin::signed(owner_acc),
                owner_did,
                token.name.clone(),
                asset_rule
            ));

            assert_ok!(Asset::transfer(
                Origin::signed(owner_acc),
                owner_did,
                token.name.clone(),
                alice_did,
                500
            ));
        })
    }

    #[test]
    fn checkpoints_fuzz_test() {
        println!("Starting");
        for i in 0..10 {
            // When fuzzing in local, feel free to bump this number to add more fuzz runs.
            with_externalities(&mut identity_owned_by_1(), || {
                let now = Utc::now();
                <timestamp::Module<Test>>::set_timestamp(now.timestamp() as u64);

                let owner_acc = 1;
                let owner_did = IdentityId::from(1u128);

                // Expected token entry
                let token = SecurityToken {
                    name: vec![0x01],
                    owner_did: owner_did.clone(),
                    total_supply: 1_000_000,
                    granularity: 1,
                    decimals: 18,
                };

                Balances::make_free_balance_be(&owner_acc, 1_000_000);
                Identity::register_did(Origin::signed(owner_acc), owner_did.clone(), vec![])
                    .expect("Could not create owner_did");

                let alice_acc = 2;
                let alice_did = IdentityId::from(2u128);

                Balances::make_free_balance_be(&alice_acc, 1_000_000);
                Identity::register_did(Origin::signed(alice_acc), alice_did.clone(), vec![])
                    .expect("Could not create alice_did");

                // Issuance is successful
                assert_ok!(Asset::create_token(
                    Origin::signed(owner_acc),
                    owner_did.clone(),
                    token.name.clone(),
                    token.name.clone(),
                    token.total_supply,
                    true
                ));

                let asset_rule = general_tm::AssetRule {
                    sender_rules: vec![],
                    receiver_rules: vec![],
                };

                // Allow all transfers
                assert_ok!(GeneralTM::add_active_rule(
                    Origin::signed(owner_acc),
                    owner_did,
                    token.name.clone(),
                    asset_rule
                ));

                let mut owner_balance: [u128; 100] = [1_000_000; 100];
                let mut alice_balance: [u128; 100] = [0; 100];
                let mut rng = rand::thread_rng();
                for j in 1..100 {
                    let transfers = rng.gen_range(0, 10);
                    owner_balance[j] = owner_balance[j - 1];
                    alice_balance[j] = alice_balance[j - 1];
                    for _k in 0..transfers {
                        if j == 1 {
                            owner_balance[0] -= 1;
                            alice_balance[0] += 1;
                        }
                        owner_balance[j] -= 1;
                        alice_balance[j] += 1;
                        assert_ok!(Asset::transfer(
                            Origin::signed(owner_acc),
                            owner_did.clone(),
                            token.name.clone(),
                            alice_did.clone(),
                            1
                        ));
                    }
                    assert_ok!(Asset::create_checkpoint(
                        Origin::signed(owner_acc),
                        owner_did.clone(),
                        token.name.clone(),
                    ));
                    let x: u64 = u64::try_from(j).unwrap();
                    assert_eq!(
                        Asset::get_balance_at(&token.name, owner_did, 0),
                        owner_balance[j]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, alice_did, 0),
                        alice_balance[j]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, owner_did, 1),
                        owner_balance[1]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, alice_did, 1),
                        alice_balance[1]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, owner_did, x - 1),
                        owner_balance[j - 1]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, alice_did, x - 1),
                        alice_balance[j - 1]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, owner_did, x),
                        owner_balance[j]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, alice_did, x),
                        alice_balance[j]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, owner_did, x + 1),
                        owner_balance[j]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, alice_did, x + 1),
                        alice_balance[j]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, owner_did, 1000),
                        owner_balance[j]
                    );
                    assert_eq!(
                        Asset::get_balance_at(&token.name, alice_did, 1000),
                        alice_balance[j]
                    );
                }
            });
            println!("Instance {} done", i);
        }
        println!("Done");
    }

    /*
     *    #[test]
     *    /// This test loads up a YAML of testcases and checks each of them
     *    fn transfer_scenarios_external() {
     *        let mut yaml_path_buf = PathBuf::new();
     *        yaml_path_buf.push(env!("CARGO_MANIFEST_DIR")); // This package's root
     *        yaml_path_buf.push("tests/asset_transfers.yml");
     *
     *        println!("Loading YAML from {:?}", yaml_path_buf);
     *
     *        let yaml_string = read_to_string(yaml_path_buf.as_path())
     *            .expect("Could not load the YAML file to a string");
     *
     *        // Parse the YAML
     *        let yaml = YamlLoader::load_from_str(&yaml_string).expect("Could not parse the YAML file");
     *
     *        let yaml = &yaml[0];
     *
     *        let now = Utc::now();
     *
     *        for case in yaml["test_cases"]
     *            .as_vec()
     *            .expect("Could not reach test_cases")
     *        {
     *            println!("Case: {:#?}", case);
     *
     *            let accounts = case["named_accounts"]
     *                .as_hash()
     *                .expect("Could not view named_accounts as a hashmap");
     *
     *            let mut externalities = if let Some(identity_owner) =
     *                accounts.get(&Yaml::String("identity-owner".to_owned()))
     *            {
     *                identity_owned_by(
     *                    identity_owner["id"]
     *                        .as_i64()
     *                        .expect("Could not get identity owner's ID") as u64,
     *                )
     *            } else {
     *                system::GenesisConfig::default()
     *                    .build_storage()
     *                    .unwrap()
     *                    .0
     *                    .into()
     *            };
     *
     *            with_externalities(&mut externalities, || {
     *                // Instantiate accounts
     *                for (name, account) in accounts {
     *                    <timestamp::Module<Test>>::set_timestamp(now.timestamp() as u64);
     *                    let name = name
     *                        .as_str()
     *                        .expect("Could not take named_accounts key as string");
     *                    let id = account["id"].as_i64().expect("id is not a number") as u64;
     *                    let balance = account["balance"]
     *                        .as_i64()
     *                        .expect("balance is not a number");
     *
     *                    println!("Preparing account {}", name);
     *
     *                    Balances::make_free_balance_be(&id, balance.clone() as u128);
     *                    println!("{}: gets {} initial balance", name, balance);
     *                    if account["issuer"]
     *                        .as_bool()
     *                        .expect("Could not check if account is an issuer")
     *                    {
     *                        assert_ok!(identity::Module::<Test>::do_create_issuer(id));
     *                        println!("{}: becomes issuer", name);
     *                    }
     *                    if account["investor"]
     *                        .as_bool()
     *                        .expect("Could not check if account is an investor")
     *                    {
     *                        assert_ok!(identity::Module::<Test>::do_create_investor(id));
     *                        println!("{}: becomes investor", name);
     *                    }
     *                }
     *
     *                // Issue tokens
     *                let tokens = case["tokens"]
     *                    .as_hash()
     *                    .expect("Could not view tokens as a hashmap");
     *
     *                for (ticker, token) in tokens {
     *                    let ticker = ticker.as_str().expect("Can't parse ticker as string");
     *                    println!("Preparing token {}:", ticker);
     *
     *                    let owner = token["owner"]
     *                        .as_str()
     *                        .expect("Can't parse owner as string");
     *
     *                    let owner_id = accounts
     *                        .get(&Yaml::String(owner.to_owned()))
     *                        .expect("Can't get owner record")["id"]
     *                        .as_i64()
     *                        .expect("Can't parse owner id as i64")
     *                        as u64;
     *                    let total_supply = token["total_supply"]
     *                        .as_i64()
     *                        .expect("Can't parse the total supply as i64")
     *                        as u128;
     *
     *                    let token_struct = SecurityToken {
     *                        name: ticker.to_owned().into_bytes(),
     *                        owner: owner_id,
     *                        total_supply,
     *                        granularity: 1,
     *                        decimals: 18,
     *                    };
     *                    println!("{:#?}", token_struct);
     *
     *                    // Check that issuing succeeds/fails as expected
     *                    if token["issuance_succeeds"]
     *                        .as_bool()
     *                        .expect("Could not check if issuance should succeed")
     *                    {
     *                        assert_ok!(Asset::create_token(
     *                            Origin::signed(token_struct.owner),
     *                            token_struct.name.clone(),
     *                            token_struct.name.clone(),
     *                            token_struct.total_supply,
     *                            true
     *                        ));
     *
     *                        // Also check that the new token matches what we asked to create
     *                        assert_eq!(
     *                            Asset::token_details(token_struct.name.clone()),
     *                            token_struct
     *                        );
     *
     *                        // Check that the issuer's balance corresponds to total supply
     *                        assert_eq!(
     *                            Asset::balance_of((token_struct.name, token_struct.owner)),
     *                            token_struct.total_supply
     *                        );
     *
     *                        // Add specified whitelist entries
     *                        let whitelists = token["whitelist_entries"]
     *                            .as_vec()
     *                            .expect("Could not view token whitelist entries as vec");
     *
     *                        for wl_entry in whitelists {
     *                            let investor = wl_entry["investor"]
     *                                .as_str()
     *                                .expect("Can't parse investor as string");
     *                            let investor_id = accounts
     *                                .get(&Yaml::String(investor.to_owned()))
     *                                .expect("Can't get investor account record")["id"]
     *                                .as_i64()
     *                                .expect("Can't parse investor id as i64")
     *                                as u64;
     *
     *                            let expiry = wl_entry["expiry"]
     *                                .as_i64()
     *                                .expect("Can't parse expiry as i64");
     *
     *                            let wl_id = wl_entry["whitelist_id"]
     *                                .as_i64()
     *                                .expect("Could not parse whitelist_id as i64")
     *                                as u32;
     *
     *                            println!(
     *                                "Token {}: processing whitelist entry for {}",
     *                                ticker, investor
     *                            );
     *
     *                            general_tm::Module::<Test>::add_to_whitelist(
     *                                Origin::signed(owner_id),
     *                                ticker.to_owned().into_bytes(),
     *                                wl_id,
     *                                investor_id,
     *                                (now + Duration::hours(expiry)).timestamp() as u64,
     *                            )
     *                            .expect("Could not create whitelist entry");
     *                        }
     *                    } else {
     *                        assert!(Asset::create_token(
     *                            Origin::signed(token_struct.owner),
     *                            token_struct.name.clone(),
     *                            token_struct.name.clone(),
     *                            token_struct.total_supply,
     *                            true
     *                        )
     *                        .is_err());
     *                    }
     *                }
     *
     *                // Set up allowances
     *                let allowances = case["allowances"]
     *                    .as_vec()
     *                    .expect("Could not view allowances as a vec");
     *
     *                for allowance in allowances {
     *                    let sender = allowance["sender"]
     *                        .as_str()
     *                        .expect("Could not view sender as str");
     *                    let sender_id = case["named_accounts"][sender]["id"]
     *                        .as_i64()
     *                        .expect("Could not view sender id as i64")
     *                        as u64;
     *                    let spender = allowance["spender"]
     *                        .as_str()
     *                        .expect("Could not view spender as str");
     *                    let spender_id = case["named_accounts"][spender]["id"]
     *                        .as_i64()
     *                        .expect("Could not view sender id as i64")
     *                        as u64;
     *                    let amount = allowance["amount"]
     *                        .as_i64()
     *                        .expect("Could not view amount as i64")
     *                        as u128;
     *                    let ticker = allowance["ticker"]
     *                        .as_str()
     *                        .expect("Could not view ticker as str");
     *                    let succeeds = allowance["succeeds"]
     *                        .as_bool()
     *                        .expect("Could not determine if allowance should succeed");
     *
     *                    if succeeds {
     *                        assert_ok!(Asset::approve(
     *                            Origin::signed(sender_id),
     *                            ticker.to_owned().into_bytes(),
     *                            spender_id,
     *                            amount,
     *                        ));
     *                    } else {
     *                        assert!(Asset::approve(
     *                            Origin::signed(sender_id),
     *                            ticker.to_owned().into_bytes(),
     *                            spender_id,
     *                            amount,
     *                        )
     *                        .is_err())
     *                    }
     *                }
     *
     *                println!("Transfers:");
     *                // Perform regular transfers
     *                let transfers = case["transfers"]
     *                    .as_vec()
     *                    .expect("Could not view transfers as vec");
     *                for transfer in transfers {
     *                    let from = transfer["from"]
     *                        .as_str()
     *                        .expect("Could not view from as str");
     *                    let from_id = case["named_accounts"][from]["id"]
     *                        .as_i64()
     *                        .expect("Could not view from_id as i64")
     *                        as u64;
     *                    let to = transfer["to"].as_str().expect("Could not view to as str");
     *                    let to_id = case["named_accounts"][to]["id"]
     *                        .as_i64()
     *                        .expect("Could not view to_id as i64")
     *                        as u64;
     *                    let amount = transfer["amount"]
     *                        .as_i64()
     *                        .expect("Could not view amount as i64")
     *                        as u128;
     *                    let ticker = transfer["ticker"]
     *                        .as_str()
     *                        .expect("Coule not view ticker as str")
     *                        .to_owned();
     *                    let succeeds = transfer["succeeds"]
     *                        .as_bool()
     *                        .expect("Could not view succeeds as bool");
     *
     *                    println!("{} of token {} from {} to {}", amount, ticker, from, to);
     *                    let ticker = ticker.into_bytes();
     *
     *                    // Get sender's investor data
     *                    let investor_data = <InvestorList<Test>>::get(from_id);
     *
     *                    println!("{}'s investor data: {:#?}", from, investor_data);
     *
     *                    if succeeds {
     *                        assert_ok!(Asset::transfer(
     *                            Origin::signed(from_id),
     *                            ticker,
     *                            to_id,
     *                            amount
     *                        ));
     *                    } else {
     *                        assert!(
     *                            Asset::transfer(Origin::signed(from_id), ticker, to_id, amount)
     *                                .is_err()
     *                        );
     *                    }
     *                }
     *
     *                println!("Approval-based transfers:");
     *                // Perform allowance transfers
     *                let transfer_froms = case["transfer_froms"]
     *                    .as_vec()
     *                    .expect("Could not view transfer_froms as vec");
     *                for transfer_from in transfer_froms {
     *                    let from = transfer_from["from"]
     *                        .as_str()
     *                        .expect("Could not view from as str");
     *                    let from_id = case["named_accounts"][from]["id"]
     *                        .as_i64()
     *                        .expect("Could not view from_id as i64")
     *                        as u64;
     *                    let spender = transfer_from["spender"]
     *                        .as_str()
     *                        .expect("Could not view spender as str");
     *                    let spender_id = case["named_accounts"][spender]["id"]
     *                        .as_i64()
     *                        .expect("Could not view spender_id as i64")
     *                        as u64;
     *                    let to = transfer_from["to"]
     *                        .as_str()
     *                        .expect("Could not view to as str");
     *                    let to_id = case["named_accounts"][to]["id"]
     *                        .as_i64()
     *                        .expect("Could not view to_id as i64")
     *                        as u64;
     *                    let amount = transfer_from["amount"]
     *                        .as_i64()
     *                        .expect("Could not view amount as i64")
     *                        as u128;
     *                    let ticker = transfer_from["ticker"]
     *                        .as_str()
     *                        .expect("Coule not view ticker as str")
     *                        .to_owned();
     *                    let succeeds = transfer_from["succeeds"]
     *                        .as_bool()
     *                        .expect("Could not view succeeds as bool");
     *
     *                    println!(
     *                        "{} of token {} from {} to {} spent by {}",
     *                        amount, ticker, from, to, spender
     *                    );
     *                    let ticker = ticker.into_bytes();
     *
     *                    // Get sender's investor data
     *                    let investor_data = <InvestorList<Test>>::get(spender_id);
     *
     *                    println!("{}'s investor data: {:#?}", from, investor_data);
     *
     *                    if succeeds {
     *                        assert_ok!(Asset::transfer_from(
     *                            Origin::signed(spender_id),
     *                            ticker,
     *                            from_id,
     *                            to_id,
     *                            amount
     *                        ));
     *                    } else {
     *                        assert!(Asset::transfer_from(
     *                            Origin::signed(from_id),
     *                            ticker,
     *                            from_id,
     *                            to_id,
     *                            amount
     *                        )
     *                        .is_err());
     *                    }
     *                }
     *            });
     *        }
     *    }
     */
}
