pub mod audit;
pub mod commit;
pub mod containment;
pub mod drift;
pub mod exec;
pub mod observer;
pub mod policy;
pub mod service;

pub mod proto {
    tonic::include_proto!("kith.daemon.v1");
}
