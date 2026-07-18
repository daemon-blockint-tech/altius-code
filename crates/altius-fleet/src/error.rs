#[derive(Debug, thiserror::Error)]
pub enum FleetError {
    #[error("graph execution failed: {0}")]
    Graph(#[from] rust_langgraph::errors::Error),
    #[error("{0} is not a recognized SVM project")]
    NotAnSvmProject(String),
    #[error("project detection failed: {0}")]
    Detect(#[from] altius_svm_detect::DetectError),
}
