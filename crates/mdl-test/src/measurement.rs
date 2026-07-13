//! Environment-qualified raw measurement records.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::num::NonZeroU32;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

mod build {
    include!(concat!(env!("OUT_DIR"), "/measurement_build_metadata.rs"));
}

/// Metadata schema version written by this harness.
pub const MEASUREMENT_SCHEMA_VERSION: u32 = 1;

/// Build and host metadata shared by one ordered set of raw samples.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MeasurementMetadata {
    schema_version: u32,
    git_revision: Box<str>,
    git_dirty: bool,
    rustc_verbose: Box<str>,
    cargo_profile: Box<str>,
    build_target: Box<str>,
    host_os: Box<str>,
    host_arch: Box<str>,
    minecraft_target: Box<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jvm: Option<JvmMeasurementMetadata>,
}

impl MeasurementMetadata {
    /// Creates compiler-only metadata using the toolchain and target captured by
    /// this crate's build script.
    #[must_use]
    pub fn new(
        git_revision: impl Into<Box<str>>,
        git_dirty: bool,
        minecraft_target: impl Into<Box<str>>,
    ) -> Self {
        Self {
            schema_version: MEASUREMENT_SCHEMA_VERSION,
            git_revision: git_revision.into(),
            git_dirty,
            rustc_verbose: build::RUSTC_VERBOSE.into(),
            cargo_profile: build::CARGO_PROFILE.into(),
            build_target: build::BUILD_TARGET.into(),
            host_os: std::env::consts::OS.into(),
            host_arch: std::env::consts::ARCH.into(),
            minecraft_target: minecraft_target.into(),
            jvm: None,
        }
    }

    /// Attaches the exact Java/server environment used by this record's samples.
    ///
    /// Callers must use this only when the recorded samples actually launched the
    /// described Java process and server artifact. Compiler-only records must leave
    /// this absent.
    #[must_use]
    pub fn with_jvm(mut self, jvm: JvmMeasurementMetadata) -> Self {
        self.jvm = Some(jvm);
        self
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub const fn git_revision(&self) -> &str {
        &self.git_revision
    }

    #[must_use]
    pub const fn git_dirty(&self) -> bool {
        self.git_dirty
    }

    #[must_use]
    pub const fn rustc_verbose(&self) -> &str {
        &self.rustc_verbose
    }

    #[must_use]
    pub const fn cargo_profile(&self) -> &str {
        &self.cargo_profile
    }

    #[must_use]
    pub const fn build_target(&self) -> &str {
        &self.build_target
    }

    #[must_use]
    pub const fn host_os(&self) -> &str {
        &self.host_os
    }

    #[must_use]
    pub const fn host_arch(&self) -> &str {
        &self.host_arch
    }

    #[must_use]
    pub const fn minecraft_target(&self) -> &str {
        &self.minecraft_target
    }

    #[must_use]
    pub const fn jvm(&self) -> Option<&JvmMeasurementMetadata> {
        self.jvm.as_ref()
    }
}

/// Exact Java and official-server identity for samples that launched Java.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct JvmMeasurementMetadata {
    java_version: Box<str>,
    server_sha256: Box<str>,
    configured_maximum_heap_mib: u32,
}

impl JvmMeasurementMetadata {
    #[must_use]
    pub fn new(
        java_version: impl Into<Box<str>>,
        server_sha256: impl Into<Box<str>>,
        configured_maximum_heap_mib: u32,
    ) -> Self {
        Self {
            java_version: java_version.into(),
            server_sha256: server_sha256.into(),
            configured_maximum_heap_mib,
        }
    }

    #[must_use]
    pub const fn java_version(&self) -> &str {
        &self.java_version
    }

    #[must_use]
    pub const fn server_sha256(&self) -> &str {
        &self.server_sha256
    }

    /// Returns the Java launch's configured `-Xmx` limit, not observed heap use.
    #[must_use]
    pub const fn configured_maximum_heap_mib(&self) -> u32 {
        self.configured_maximum_heap_mib
    }
}

/// Configuration counterbalancing used by one measurement protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum MeasurementConfigurationSchedule {
    /// Rotate the declared configuration order left by the zero-based iteration.
    RotateLeftPerIteration,
}

/// Reproducible ordering and repetition policy for one fixture.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MeasurementProtocol {
    fixture: Box<str>,
    warmup_iterations: u32,
    sample_iterations: NonZeroU32,
    subject_order: Box<[Box<str>]>,
    configuration_order: Box<[Box<str>]>,
    configuration_schedule: MeasurementConfigurationSchedule,
}

impl MeasurementProtocol {
    #[must_use]
    pub fn new(
        fixture: impl Into<Box<str>>,
        warmup_iterations: u32,
        sample_iterations: NonZeroU32,
        subject_order: impl IntoIterator<Item = impl Into<Box<str>>>,
        configuration_order: impl IntoIterator<Item = impl Into<Box<str>>>,
    ) -> Self {
        Self {
            fixture: fixture.into(),
            warmup_iterations,
            sample_iterations,
            subject_order: subject_order
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            configuration_order: configuration_order
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            configuration_schedule: MeasurementConfigurationSchedule::RotateLeftPerIteration,
        }
    }

    #[must_use]
    pub const fn fixture(&self) -> &str {
        &self.fixture
    }

    #[must_use]
    pub const fn warmup_iterations(&self) -> u32 {
        self.warmup_iterations
    }

    #[must_use]
    pub const fn sample_iterations(&self) -> NonZeroU32 {
        self.sample_iterations
    }

    #[must_use]
    pub fn subject_order(&self) -> impl ExactSizeIterator<Item = &str> + Clone {
        self.subject_order.iter().map(std::convert::AsRef::as_ref)
    }

    #[must_use]
    pub fn configuration_order(&self) -> impl ExactSizeIterator<Item = &str> + Clone {
        self.configuration_order
            .iter()
            .map(std::convert::AsRef::as_ref)
    }

    #[must_use]
    pub const fn configuration_schedule(&self) -> MeasurementConfigurationSchedule {
        self.configuration_schedule
    }

    /// Returns the exact counterbalanced configuration order for one iteration.
    #[must_use]
    pub fn configurations_for_iteration(
        &self,
        iteration: u32,
    ) -> impl ExactSizeIterator<Item = &str> + Clone {
        let len = self.configuration_order.len();
        let offset = if len == 0 {
            0
        } else {
            usize::try_from(iteration).unwrap_or(usize::MAX) % len
        };
        (0..len).map(move |position| {
            let index = (offset + position) % len;
            self.configuration_order[index].as_ref()
        })
    }
}

/// Outcome of one ordered measurement subject.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum MeasurementSampleStatus {
    Completed { elapsed_nanoseconds: u64 },
    Skipped { reason: Box<str> },
}

/// One raw observation; samples are never averaged inside the harness record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MeasurementSample {
    ordinal: u64,
    subject: Box<str>,
    configuration: Box<str>,
    outcome: MeasurementSampleStatus,
}

impl MeasurementSample {
    #[must_use]
    pub fn completed(
        ordinal: u64,
        subject: impl Into<Box<str>>,
        configuration: impl Into<Box<str>>,
        elapsed_nanoseconds: u64,
    ) -> Self {
        Self {
            ordinal,
            subject: subject.into(),
            configuration: configuration.into(),
            outcome: MeasurementSampleStatus::Completed {
                elapsed_nanoseconds,
            },
        }
    }

    #[must_use]
    pub fn skipped(
        ordinal: u64,
        subject: impl Into<Box<str>>,
        configuration: impl Into<Box<str>>,
        reason: impl Into<Box<str>>,
    ) -> Self {
        Self {
            ordinal,
            subject: subject.into(),
            configuration: configuration.into(),
            outcome: MeasurementSampleStatus::Skipped {
                reason: reason.into(),
            },
        }
    }

    #[must_use]
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    #[must_use]
    pub const fn subject(&self) -> &str {
        &self.subject
    }

    #[must_use]
    pub const fn configuration(&self) -> &str {
        &self.configuration
    }

    #[must_use]
    pub const fn outcome(&self) -> &MeasurementSampleStatus {
        &self.outcome
    }
}

/// Harness-owned metadata, protocol, and raw observations for one fixture.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MeasurementRecord {
    metadata: MeasurementMetadata,
    protocol: MeasurementProtocol,
    samples: Box<[MeasurementSample]>,
}

/// A raw sample sequence that contradicts its declared measurement protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MeasurementRecordError {
    /// A protocol cannot define a useful sample schedule without a subject.
    EmptySubjectOrder,
    /// A protocol cannot define a useful sample schedule without a configuration.
    EmptyConfigurationOrder,
    /// The declared schedule cannot be represented by the record's count domain.
    SampleCountOverflow,
    /// The number of supplied samples disagrees with the complete declared schedule.
    SampleCountMismatch { expected: u64, actual: u64 },
    /// A sample's ordinal is not its exact zero-based position.
    OrdinalMismatch {
        index: u64,
        expected: u64,
        actual: u64,
    },
    /// A sample does not name the subject scheduled at its position.
    SubjectMismatch {
        ordinal: u64,
        expected: Box<str>,
        actual: Box<str>,
    },
    /// A sample does not name the counterbalanced configuration scheduled at its position.
    ConfigurationMismatch {
        ordinal: u64,
        expected: Box<str>,
        actual: Box<str>,
    },
}

impl fmt::Display for MeasurementRecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySubjectOrder => formatter.write_str("measurement protocol has no subjects"),
            Self::EmptyConfigurationOrder => {
                formatter.write_str("measurement protocol has no configurations")
            }
            Self::SampleCountOverflow => {
                formatter.write_str("measurement protocol sample count overflowed u64")
            }
            Self::SampleCountMismatch { expected, actual } => write!(
                formatter,
                "measurement protocol requires {expected} samples but received {actual}"
            ),
            Self::OrdinalMismatch {
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "measurement sample at index {index} has ordinal {actual}, expected {expected}"
            ),
            Self::SubjectMismatch {
                ordinal,
                expected,
                actual,
            } => write!(
                formatter,
                "measurement sample {ordinal} has subject {actual:?}, expected {expected:?}"
            ),
            Self::ConfigurationMismatch {
                ordinal,
                expected,
                actual,
            } => write!(
                formatter,
                "measurement sample {ordinal} has configuration {actual:?}, expected {expected:?}"
            ),
        }
    }
}

impl std::error::Error for MeasurementRecordError {}

impl MeasurementRecord {
    /// Validates and owns one complete raw sample schedule.
    ///
    /// # Errors
    ///
    /// Returns a typed error when either declared order is empty, the expected
    /// count overflows, or any count, ordinal, subject, or configuration disagrees
    /// with the protocol's exact counterbalanced schedule.
    pub fn new(
        metadata: MeasurementMetadata,
        protocol: MeasurementProtocol,
        samples: Vec<MeasurementSample>,
    ) -> Result<Self, MeasurementRecordError> {
        validate_measurement_samples(&protocol, &samples)?;
        Ok(Self {
            metadata,
            protocol,
            samples: samples.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn metadata(&self) -> &MeasurementMetadata {
        &self.metadata
    }

    #[must_use]
    pub const fn protocol(&self) -> &MeasurementProtocol {
        &self.protocol
    }

    #[must_use]
    pub fn samples(&self) -> &[MeasurementSample] {
        &self.samples
    }

    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        MeasurementMetadata,
        MeasurementProtocol,
        Box<[MeasurementSample]>,
    ) {
        (self.metadata, self.protocol, self.samples)
    }

    /// Serializes one self-contained JSON value followed by a newline.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn write_json_line(&self, mut output: impl Write) -> io::Result<()> {
        serde_json::to_writer(&mut output, self).map_err(io::Error::other)?;
        output.write_all(b"\n")
    }

    /// Returns one self-contained JSON value followed by a newline.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn to_json_line(&self) -> serde_json::Result<String> {
        serde_json::to_string(self).map(|mut json| {
            json.push('\n');
            json
        })
    }
}

fn validate_measurement_samples(
    protocol: &MeasurementProtocol,
    samples: &[MeasurementSample],
) -> Result<(), MeasurementRecordError> {
    let subject_count = u64::try_from(protocol.subject_order.len())
        .map_err(|_| MeasurementRecordError::SampleCountOverflow)?;
    if subject_count == 0 {
        return Err(MeasurementRecordError::EmptySubjectOrder);
    }
    let configuration_count = u64::try_from(protocol.configuration_order.len())
        .map_err(|_| MeasurementRecordError::SampleCountOverflow)?;
    if configuration_count == 0 {
        return Err(MeasurementRecordError::EmptyConfigurationOrder);
    }
    let samples_per_iteration = subject_count
        .checked_mul(configuration_count)
        .ok_or(MeasurementRecordError::SampleCountOverflow)?;
    let expected_count = samples_per_iteration
        .checked_mul(u64::from(protocol.sample_iterations.get()))
        .ok_or(MeasurementRecordError::SampleCountOverflow)?;
    let actual_count =
        u64::try_from(samples.len()).map_err(|_| MeasurementRecordError::SampleCountOverflow)?;
    if actual_count != expected_count {
        return Err(MeasurementRecordError::SampleCountMismatch {
            expected: expected_count,
            actual: actual_count,
        });
    }

    let configuration_count = protocol.configuration_order.len();
    let samples_per_iteration = usize::try_from(samples_per_iteration)
        .map_err(|_| MeasurementRecordError::SampleCountOverflow)?;
    for (index, sample) in samples.iter().enumerate() {
        let ordinal =
            u64::try_from(index).map_err(|_| MeasurementRecordError::SampleCountOverflow)?;
        if sample.ordinal != ordinal {
            return Err(MeasurementRecordError::OrdinalMismatch {
                index: ordinal,
                expected: ordinal,
                actual: sample.ordinal,
            });
        }
        let iteration = index / samples_per_iteration;
        let within_iteration = index % samples_per_iteration;
        let subject_index = within_iteration / configuration_count;
        let configuration_position = within_iteration % configuration_count;
        let expected_subject = protocol.subject_order[subject_index].as_ref();
        if sample.subject.as_ref() != expected_subject {
            return Err(MeasurementRecordError::SubjectMismatch {
                ordinal,
                expected: expected_subject.into(),
                actual: sample.subject.clone(),
            });
        }
        let configuration_offset = iteration % configuration_count;
        let configuration_index =
            (configuration_offset + configuration_position) % configuration_count;
        let expected_configuration = protocol.configuration_order[configuration_index].as_ref();
        if sample.configuration.as_ref() != expected_configuration {
            return Err(MeasurementRecordError::ConfigurationMismatch {
                ordinal,
                expected: expected_configuration.into(),
                actual: sample.configuration.clone(),
            });
        }
    }
    Ok(())
}

/// Computes the exact lowercase SHA-256 digest of one server artifact.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be read.
pub fn sha256_file(path: impl AsRef<Path>) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> MeasurementMetadata {
        MeasurementMetadata::new("deadbeef", true, "minecraft-java-26.2")
    }

    fn protocol(sample_iterations: u32) -> MeasurementProtocol {
        MeasurementProtocol::new(
            "tiny",
            1,
            NonZeroU32::new(sample_iterations).unwrap(),
            ["optimizer", "emission"],
            ["core-none+mc-none", "core-baseline+mc-baseline"],
        )
    }

    fn samples(protocol: &MeasurementProtocol) -> Vec<MeasurementSample> {
        let mut samples = Vec::new();
        let mut ordinal = 0;
        for iteration in 0..protocol.sample_iterations().get() {
            for subject in protocol.subject_order() {
                for configuration in protocol.configurations_for_iteration(iteration) {
                    samples.push(MeasurementSample::completed(
                        ordinal,
                        subject,
                        configuration,
                        41 + ordinal,
                    ));
                    ordinal += 1;
                }
            }
        }
        samples
    }

    #[test]
    fn raw_records_preserve_order_and_omit_absent_jvm_metadata() {
        let protocol = protocol(2);
        let samples = samples(&protocol);
        let record = MeasurementRecord::new(metadata(), protocol, samples).unwrap();

        assert_eq!(record.metadata().schema_version(), 1);
        assert!(record.metadata().jvm().is_none());
        assert!(record.metadata().rustc_verbose().contains("rustc"));
        assert!(!record.metadata().cargo_profile().is_empty());
        assert!(!record.metadata().build_target().is_empty());
        assert_eq!(record.metadata().minecraft_target(), "minecraft-java-26.2");
        assert_eq!(record.protocol().subject_order().next(), Some("optimizer"));
        assert_eq!(
            record.protocol().configuration_order().next(),
            Some("core-none+mc-none")
        );
        assert_eq!(
            record
                .protocol()
                .configurations_for_iteration(1)
                .collect::<Vec<_>>(),
            vec!["core-baseline+mc-baseline", "core-none+mc-none"]
        );
        assert_eq!(record.samples()[0].ordinal(), 0);
        let json = record.to_json_line().unwrap();
        assert!(!json.contains("\"jvm\""));
        assert!(json.contains("\"build_target\""));
        assert!(json.contains("\"minecraft_target\""));
        assert!(json.contains("\"subject_order\""));
        assert!(json.contains("\"configuration_schedule\":\"rotate-left-per-iteration\""));
        assert!(json.ends_with('\n'));
        assert!(json.find("\"ordinal\":0").unwrap() < json.find("\"ordinal\":1").unwrap());
    }

    #[test]
    fn jvm_metadata_and_into_parts_remain_typed() {
        let jvm = JvmMeasurementMetadata::new("openjdk 25", "0123", 1024);
        assert_eq!(jvm.configured_maximum_heap_mib(), 1024);
        let metadata = MeasurementMetadata::new("deadbeef", false, "minecraft-java-26.2")
            .with_jvm(jvm.clone());
        assert_eq!(metadata.jvm(), Some(&jvm));
        let protocol = MeasurementProtocol::new(
            "normal",
            0,
            NonZeroU32::MIN,
            ["server-execution"],
            ["core-baseline+mc-baseline"],
        );
        let samples = vec![MeasurementSample::completed(
            0,
            "server-execution",
            "core-baseline+mc-baseline",
            1,
        )];
        let record = MeasurementRecord::new(metadata.clone(), protocol.clone(), samples).unwrap();
        let (owned_metadata, owned_protocol, samples) = record.into_parts();
        assert_eq!(owned_metadata, metadata);
        assert_eq!(owned_protocol, protocol);
        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn record_rejects_count_and_ordinal_mismatches() {
        let protocol = protocol(1);
        let mut short_samples = samples(&protocol);
        short_samples.pop();
        assert_eq!(
            MeasurementRecord::new(metadata(), protocol.clone(), short_samples),
            Err(MeasurementRecordError::SampleCountMismatch {
                expected: 4,
                actual: 3,
            })
        );

        let mut wrong_ordinal = samples(&protocol);
        wrong_ordinal[2].ordinal = 9;
        assert_eq!(
            MeasurementRecord::new(metadata(), protocol, wrong_ordinal),
            Err(MeasurementRecordError::OrdinalMismatch {
                index: 2,
                expected: 2,
                actual: 9,
            })
        );
    }

    #[test]
    fn record_rejects_subject_and_rotated_configuration_mismatches() {
        let protocol = protocol(2);
        let mut wrong_subject = samples(&protocol);
        wrong_subject[4].subject = "emission".into();
        assert_eq!(
            MeasurementRecord::new(metadata(), protocol.clone(), wrong_subject),
            Err(MeasurementRecordError::SubjectMismatch {
                ordinal: 4,
                expected: "optimizer".into(),
                actual: "emission".into(),
            })
        );

        let mut wrong_configuration = samples(&protocol);
        wrong_configuration[4].configuration = "core-none+mc-none".into();
        assert_eq!(
            MeasurementRecord::new(metadata(), protocol, wrong_configuration),
            Err(MeasurementRecordError::ConfigurationMismatch {
                ordinal: 4,
                expected: "core-baseline+mc-baseline".into(),
                actual: "core-none+mc-none".into(),
            })
        );
    }

    #[test]
    fn record_rejects_empty_declared_orders() {
        let no_subjects = MeasurementProtocol::new(
            "tiny",
            0,
            NonZeroU32::MIN,
            std::iter::empty::<&str>(),
            ["none"],
        );
        assert_eq!(
            MeasurementRecord::new(metadata(), no_subjects, vec![]),
            Err(MeasurementRecordError::EmptySubjectOrder)
        );
        let no_configurations = MeasurementProtocol::new(
            "tiny",
            0,
            NonZeroU32::MIN,
            ["optimizer"],
            std::iter::empty::<&str>(),
        );
        assert_eq!(
            MeasurementRecord::new(metadata(), no_configurations, vec![]),
            Err(MeasurementRecordError::EmptyConfigurationOrder)
        );
    }

    #[test]
    fn server_artifact_hash_uses_exact_sha256_bytes() {
        let path = std::env::temp_dir().join(format!(
            "mdl-measurement-sha256-{}-{}.bin",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        std::fs::write(&path, b"abc").unwrap();
        let digest = sha256_file(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
