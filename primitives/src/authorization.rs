use crate::identity_id::IdentityId;
use codec::{Decode, Encode};
use rstd::prelude::Vec;

/// Authorization data for two step prcoesses.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum AuthorizationData {
    /// Authorization to transfer a ticker
    TransferTicker(Vec<u8>),
    /// Any other authorization
    Custom(Vec<u8>),
    /// No authorization data
    None,
}

impl Default for AuthorizationData {
    fn default() -> Self {
        AuthorizationData::None
    }
}

/// Authorization struct
#[derive(Encode, Decode, Default, Clone, PartialEq, Debug)]
pub struct Authorization<U> {
    /// Enum that contains authorization type and data
    pub authorization_data: AuthorizationData,

    /// Identity of the organization/individual that added this authorization
    pub authorized_by: IdentityId,

    /// time when this authorization expires. optional.
    pub expiry: Option<U>,

    // Extra data to allow iterating over the authorizations.
    /// Authorization number of the next Authorization.
    /// Authorization number starts with 1.
    pub next_authorization: u64,
    /// Authorization number of the previous Authorization.
    /// Authorization number starts with 1.
    pub previous_authorization: u64,
}