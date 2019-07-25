//! A runtime module providing a unique ticker registry
use crate::{asset, erc20};
use parity_codec::{Decode, Encode};
use rstd::prelude::*;
use crate::utils;
use system::{self, ensure_signed};
use runtime_primitives::traits::{As, CheckedAdd, CheckedSub, Convert, StaticLookup};
use support::{decl_module, decl_storage, dispatch::Result, ensure, StorageMap};

/// The module's configuration trait.
pub trait Trait: system::Trait + utils::Trait + asset::Trait + erc20::Trait{

}

#[derive(parity_codec::Encode, parity_codec::Decode, Default, Clone, PartialEq, Debug)]
pub struct Instruction<U> {
    /// Total amount to be sold
    amount: U,
    ticker: Vec<u8>,
    regulated: bool,
}

decl_storage! {
    trait Store for Module<T: Trait> as Settlement {
        pub SettlementContracts get(settlement_contracts): map (Vec<u8>, u64) => T::AccountId;
        pub NumberOfContracts get(number_of_contracts): map Vec<u8> => u64;
        pub ContractPosition get(contract_position): map (Vec<u8>, T::AccountId) => u64;
        pub DepositedSecurityBalance get(deposited_security_balance): map (Vec<u8>, T::AccountId) => T::TokenBalance;
        pub DepositedERC20Balance get(deposited_erc20_balance): map (Vec<u8>, T::AccountId) => T::TokenBalance;
        //pub AvailableSecurityBalance get(deposited_security_balance): map (Vec<u8>, T::AccountId) => T::TokenBalance;
        //pub AvailableERC20Balance get(deposited_erc20_balance): map (Vec<u8>, T::AccountId) => T::TokenBalance;
        Instructions get(instructions): map (T::AccountId, u64) => Instruction<T::TokenBalance>;
        InstructionsCount get(instructions_count): map T::AccountId => u64;
        //InstructionsPosition get(instructions_position): map (T::AccountId, u64) => u64;
    }
}

decl_module! {
    /// The module declaration.
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {

        pub fn add_settlement_contract(origin, _ticker: Vec<u8>, _contract: T::AccountId) -> Result {
            let sender = ensure_signed(origin)?;
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            ensure!(Self::is_owner(ticker.clone(),sender.clone()),"Sender must be the token owner");
            ensure!(!<ContractPosition<T>>::exists((ticker.clone(), _contract.clone())), "Contract already added");
            let number_of_settlement_contracts = Self::number_of_contracts(ticker.clone()).checked_add(1).ok_or("overflow in increasing number of contracts")?;
            <SettlementContracts<T>>::insert((ticker.clone(), number_of_settlement_contracts.clone()), _contract.clone());
            <NumberOfContracts<T>>::insert(
                ticker.clone(),
                number_of_settlement_contracts
            );
            <ContractPosition<T>>::insert((ticker.clone(), _contract.clone()), number_of_settlement_contracts);
            Ok(())
        }

        pub fn remove_settlement_contract(origin, _ticker: Vec<u8>, _contract: T::AccountId) -> Result {
            let sender = ensure_signed(origin)?;
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            ensure!(Self::is_owner(ticker.clone(),sender.clone()),"Sender must be the token owner");
            ensure!(<ContractPosition<T>>::exists((ticker.clone(), _contract.clone())), "Contract not added");
            let contract_pos = Self::contract_position((ticker.clone(), _contract.clone()));
            let last_contract = Self::settlement_contracts((ticker.clone(), Self::number_of_contracts(ticker.clone())));
            <SettlementContracts<T>>::insert((ticker.clone(), contract_pos), last_contract.clone());
            <NumberOfContracts<T>>::insert(
                ticker.clone(),
                Self::number_of_contracts(ticker.clone()).checked_sub(1).ok_or("underflow number of contracts")?
            );
            <ContractPosition<T>>::remove((ticker.clone(), _contract.clone()));
            <ContractPosition<T>>::insert((ticker.clone(), last_contract.clone()), contract_pos);
            Ok(())
        }

        pub fn deposit_security_tokens(origin, _ticker: Vec<u8>, _amount: T::TokenBalance) -> Result {
            let sender = ensure_signed(origin)?;
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            //<asset::Module<T>>::_transfer(ticker.clone(), sender.clone(), 0, _amount.clone());
            let balance = <asset::BalanceOf<T>>::get((ticker.clone(), sender.clone()));
            let new_balance = balance.checked_sub(&_amount).ok_or("Overflow calculating new owner balance")?;
            let new_deposit_balance = Self::deposited_security_balance((ticker.clone(), sender.clone())).checked_add(&_amount).ok_or("Overflow in deposit balance")?;
            //let new_available_balance = Self::available_security_balance((ticker.clone(), sender.clone())).checked_add(&_amount).ok_or("Overflow in available balance")?;
            <asset::BalanceOf<T>>::insert((ticker.clone(), sender.clone()), new_balance);
            <DepositedSecurityBalance<T>>::insert((ticker.clone(), sender.clone()), new_deposit_balance);
            //<AvailableSecurityBalance<T>>::insert((ticker.clone(), sender.clone()), new_available_balance);
            Ok(())
        }

        pub fn withdraw_security_tokens(origin, _ticker: Vec<u8>, _amount: T::TokenBalance) -> Result {
            let sender = ensure_signed(origin)?;
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            let new_deposit_balance = Self::deposited_security_balance((ticker.clone(), sender.clone())).checked_sub(&_amount).ok_or("Underflow in deposit balance")?;
            let new_balance = <asset::BalanceOf<T>>::get((ticker.clone(), sender.clone())).checked_add(&_amount).ok_or("Overflow calculating new owner balance")?;
            <DepositedSecurityBalance<T>>::insert((ticker.clone(), sender.clone()), new_deposit_balance);
            <asset::BalanceOf<T>>::insert((ticker.clone(), sender.clone()), new_balance);
            Ok(())
        }

        pub fn deposit_erc20_tokens(origin, _ticker: Vec<u8>, _amount: T::TokenBalance) -> Result {
            let sender = ensure_signed(origin)?;
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            //<asset::Module<T>>::_transfer(ticker.clone(), sender.clone(), 0, _amount.clone());
            let balance = <erc20::BalanceOf<T>>::get((ticker.clone(), sender.clone()));
            let new_balance = balance.checked_sub(&_amount).ok_or("Overflow calculating new owner balance")?;
            let new_deposit_balance = Self::deposited_erc20_balance((ticker.clone(), sender.clone())).checked_add(&_amount).ok_or("Overflow in deposit balance")?;
            <erc20::BalanceOf<T>>::insert((ticker.clone(), sender.clone()), new_balance);
            <DepositedERC20Balance<T>>::insert((ticker.clone(), sender.clone()), new_deposit_balance);
            Ok(())
        }

        pub fn withdraw_erc20_tokens(origin, _ticker: Vec<u8>, _amount: T::TokenBalance) -> Result {
            let sender = ensure_signed(origin)?;
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            let new_deposit_balance = Self::deposited_erc20_balance((ticker.clone(), sender.clone())).checked_sub(&_amount).ok_or("Underflow in deposit balance")?;
            let new_balance = <erc20::BalanceOf<T>>::get((ticker.clone(), sender.clone())).checked_add(&_amount).ok_or("Overflow calculating new owner balance")?;
            <DepositedERC20Balance<T>>::insert((ticker.clone(), sender.clone()), new_deposit_balance);
            <erc20::BalanceOf<T>>::insert((ticker.clone(), sender.clone()), new_balance);
            Ok(())
        }

        pub fn add_instruction_fixed(origin, _ticker: Vec<u8>, _regulated: bool, _amount: T::TokenBalance /* settlement_criteria */) -> Result {
            let sender = ensure_signed(origin)?;
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            if _regulated {
                let new_deposit_balance = Self::deposited_security_balance((ticker.clone(), sender.clone())).checked_sub(&_amount).ok_or("Underflow in deposit balance")?;
                <DepositedSecurityBalance<T>>::insert((ticker.clone(), sender.clone()), new_deposit_balance);
            } else {
                let new_deposit_balance = Self::deposited_erc20_balance((ticker.clone(), sender.clone())).checked_sub(&_amount).ok_or("Underflow in deposit balance")?;
                <DepositedERC20Balance<T>>::insert((ticker.clone(), sender.clone()), new_deposit_balance);
            }
            let new_instruction = Instruction {
                amount: _amount,
                ticker: _ticker,
                regulated: _regulated,
            };
            let new_count = <InstructionsCount<T>>::get(sender.clone())
                .checked_add(1)
                .ok_or("Could not add 1 to dividend count")?;
            //<InstructionsPosition<T>>::insert((sender.clone(), new_count.clone()), new_count.clone());
            <Instructions<T>>::insert((sender.clone(), new_count.clone()), new_instruction);
            <InstructionsCount<T>>::insert(sender.clone(), new_count);
            Ok(())
        }

        pub fn settle_instruction(origin, _ticker: Vec<u8>, _instruction_id: u64) -> Result {
            let sender = ensure_signed(origin)?;
            let ticker = utils::bytes_to_upper(_ticker.as_slice());
            ensure!(<Instructions<T>>::exists((sender.clone(), _instruction_id)), "No instruction for supplied ticker and ID");
            //Do settlement magic
            <Instructions<T>>::remove((sender.clone(), _instruction_id));
            Ok(())
        }
       // T::Lookup::unlookup(_contract.clone())
    }
}

impl<T: Trait> Module<T> {
    pub fn is_owner(_ticker: Vec<u8>, sender: T::AccountId) -> bool {
        let ticker = utils::bytes_to_upper(_ticker.as_slice());
        <asset::Module<T>>::_is_owner(ticker.clone(), sender)
    }
}

/// tests for this module
#[cfg(test)]
mod tests {
    use super::*;

    use primitives::{Blake2Hasher, H256};
    use runtime_io::with_externalities;
    use runtime_primitives::{
        testing::{Digest, DigestItem, Header},
        traits::{BlakeTwo256, IdentityLookup},
        BuildStorage,
    };
    use support::{assert_ok, impl_outer_origin};

    impl_outer_origin! {
        pub enum Origin for Test {}
    }

    // For testing the module, we construct most of a mock runtime. This means
    // first constructing a configuration type (`Test`) which `impl`s each of the
    // configuration traits of modules we want to use.
    #[derive(Clone, Eq, PartialEq)]
    pub struct Test;
    impl system::Trait for Test {
        type Origin = Origin;
        type Index = u64;
        type BlockNumber = u64;
        type Hash = H256;
        type Hashing = BlakeTwo256;
        type Digest = Digest;
        type AccountId = u64;
        type Lookup = IdentityLookup<Self::AccountId>;
        type Header = Header;
        type Event = ();
        type Log = DigestItem;
    }
    impl Trait for Test {}

}
