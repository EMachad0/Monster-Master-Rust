use crate::{StdbSubscriptionDriver, connection::stdb_connection_driver::StdbConnectionDriver};

/// Produces the connection and subscription drivers when the plugin is built.
pub trait StdbBuilder: 'static + Send + Sync {
    type Cd: StdbConnectionDriver;
    type Sd: StdbSubscriptionDriver<Conn = <Self::Cd as StdbConnectionDriver>::Conn>;

    fn build_cd(&self) -> Self::Cd;
    fn build_sd(&self) -> Self::Sd;
}

/// A builder over two already-constructed drivers, handing back a clone of each on build. The SDK
/// drivers are not `Clone` and are built from parameters by `SdkBuilder`; this is for drivers that
/// are cheap to clone.
pub struct Drivers<Cd, Sd> {
    conn_driver: Cd,
    sub_driver: Sd,
}

impl<Cd, Sd> Drivers<Cd, Sd> {
    pub fn new(conn_driver: Cd, sub_driver: Sd) -> Self {
        Self {
            conn_driver,
            sub_driver,
        }
    }
}

impl<Cd, Sd> StdbBuilder for Drivers<Cd, Sd>
where
    Cd: StdbConnectionDriver + Clone,
    Sd: StdbSubscriptionDriver<Conn = Cd::Conn> + Clone,
{
    type Cd = Cd;
    type Sd = Sd;

    fn build_cd(&self) -> Cd {
        self.conn_driver.clone()
    }

    fn build_sd(&self) -> Sd {
        self.sub_driver.clone()
    }
}
