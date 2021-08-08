use crate::ClientId;

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionPermission {
    /// All clients are allowed, except denied ones.
    All,
    /// Only clients in the allow list can connect.
    OnlyAllowed,
    /// No clients can connect.
    None,
}

pub struct ConnectionControl<C> {
    allow_clients: HashSet<C>,
    deny_clients: HashSet<C>,
    connection_permission: ConnectionPermission,
}

impl<C: ClientId> ConnectionControl<C> {
    pub fn new(connection_permission: ConnectionPermission) -> Self {
        Self {
            connection_permission,
            allow_clients: HashSet::new(),
            deny_clients: HashSet::new(),
        }
    }

    pub fn allow_client(&mut self, client_id: &C) {
        self.allow_clients.insert(*client_id);
        self.deny_clients.remove(client_id);
    }

    pub fn deny_client(&mut self, client_id: &C) {
        self.deny_clients.insert(*client_id);
        self.allow_clients.remove(client_id);
    }

    pub fn set_connection_permission(&mut self, connection_permission: ConnectionPermission) {
        self.connection_permission = connection_permission;
    }

    pub fn connection_permission(&self) -> &ConnectionPermission {
        &self.connection_permission
    }

    pub fn is_client_permitted(&self, client_id: &C) -> bool {
        if self.deny_clients.contains(client_id) {
            return false;
        }

        match self.connection_permission {
            ConnectionPermission::All => true,
            ConnectionPermission::OnlyAllowed => self.allow_clients.contains(&client_id),
            ConnectionPermission::None => false,
        }
    }

    pub fn allowed_clients(&self) -> Vec<C> {
        self.allow_clients.iter().copied().collect()
    }

    pub fn denied_clients(&self) -> Vec<C> {
        self.deny_clients.iter().copied().collect()
    }
}

