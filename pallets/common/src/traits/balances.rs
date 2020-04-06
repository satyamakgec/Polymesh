use crate::traits::{identity::IdentityTrait, CommonTrait, NegativeImbalance};

use codec::{Decode, Encode};
use frame_support::{
    decl_event,
    dispatch::DispatchError,
    traits::{ExistenceRequirement, Get, OnUnbalanced, StoredMap, WithdrawReason, WithdrawReasons},
};
use frame_system::{self as system};
use polymesh_primitives::AccountKey;
use sp_runtime::{traits::Saturating, RuntimeDebug};
use sp_std::ops::BitOr;

#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
pub struct Memo(pub [u8; 32]);

impl Default for Memo {
    fn default() -> Self {
        Memo([0u8; 32])
    }
}

// POLYMESH-NOTE: Make `AccountData` public to access it from the outside module.
/// All balance information for an account.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Default, RuntimeDebug)]
pub struct AccountData<Balance> {
    /// Non-reserved part of the balance. There may still be restrictions on this, but it is the
    /// total pool what may in principle be transferred, reserved and used for tipping.
    ///
    /// This is the only balance that matters in terms of most operations on tokens. It
    /// alone is used to determine the balance when in the contract execution environment.
    pub free: Balance,
    /// Balance which is reserved and may not be used at all.
    ///
    /// This can still get slashed, but gets slashed last of all.
    ///
    /// This balance is a 'reserve' balance that other subsystems use in order to set aside tokens
    /// that are still 'owned' by the account holder, but which are suspendable.
    pub reserved: Balance,
    /// The amount that `free` may not drop below when withdrawing for *anything except transaction
    /// fee payment*.
    pub misc_frozen: Balance,
    /// The amount that `free` may not drop below when withdrawing specifically for transaction
    /// fee payment.
    pub fee_frozen: Balance,
}

impl<Balance: Saturating + Copy + Ord> AccountData<Balance> {
    /// How much this account's balance can be reduced for the given `reasons`.
    pub fn usable(&self, reasons: Reasons) -> Balance {
        self.free.saturating_sub(self.frozen(reasons))
    }
    /// The amount that this account's free balance may not be reduced beyond for the given
    /// `reasons`.
    pub fn frozen(&self, reasons: Reasons) -> Balance {
        match reasons {
            Reasons::All => self.misc_frozen.max(self.fee_frozen),
            Reasons::Misc => self.misc_frozen,
            Reasons::Fee => self.fee_frozen,
        }
    }
    /// The total balance in this account including any that is reserved and ignoring any frozen.
    pub fn total(&self) -> Balance {
        self.free.saturating_add(self.reserved)
    }
}

/// Simplified reasons for withdrawing balance.
#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, RuntimeDebug)]
pub enum Reasons {
    /// Paying system transaction fees.
    Fee = 0,
    /// Any reason other than paying system transaction fees.
    Misc = 1,
    /// Any reason at all.
    All = 2,
}

impl From<WithdrawReasons> for Reasons {
    fn from(r: WithdrawReasons) -> Reasons {
        if r == WithdrawReasons::from(WithdrawReason::TransactionPayment) {
            Reasons::Fee
        } else if r.contains(WithdrawReason::TransactionPayment) {
            Reasons::All
        } else {
            Reasons::Misc
        }
    }
}

impl BitOr for Reasons {
    type Output = Reasons;
    fn bitor(self, other: Reasons) -> Reasons {
        if self == other {
            return self;
        }
        Reasons::All
    }
}

decl_event!(
    pub enum Event<T> where
    <T as system::Trait>::AccountId,
    <T as CommonTrait>::Balance
    {
        /// An account was created with some free balance.
        Endowed(AccountId, Balance),
        /// Some amount was deposited (e.g. for transaction fees).
        Deposit(AccountId, Balance),
        /// Transfer succeeded (from, to, value).
        Transfer(AccountId, AccountId, Balance),
        /// A balance was set by root (who, free, reserved).
        BalanceSet(AccountId, Balance, Balance),
        /// Transfer succeded with a memo.
        TransferWithMemo(AccountId, AccountId, Balance, Memo),
    }
);

pub trait Trait: CommonTrait {
    /// The means of storing the balances of an account.
    type AccountStore: StoredMap<Self::AccountId, AccountData<Self::Balance>>;

    /// Handler for the unbalanced reduction when removing a dust account.
    type DustRemoval: OnUnbalanced<NegativeImbalance<Self>>;

    /// The overarching event type.
    type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;

    /// This type is no longer needed but kept for compatibility reasons.
    /// The minimum amount required to keep an account open.
    type ExistentialDeposit: Get<<Self as CommonTrait>::Balance>;

    /// Used to charge fee to identity rather than user directly
    type Identity: IdentityTrait;

    /// Used to check if an account is linked to a CDD'd identity
    type CddChecker: CheckCdd;
}

pub trait BalancesTrait<A, B, NI> {
    fn withdraw(
        who: &A,
        value: B,
        reasons: WithdrawReasons,
        _liveness: ExistenceRequirement,
    ) -> sp_std::result::Result<NI, DispatchError>;
}

pub trait CheckCdd {
    fn check_key_cdd(key: &AccountKey) -> bool;
}