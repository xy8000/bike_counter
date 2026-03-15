pub trait MeasurementProvider {
    fn get_measurements(&self, query: MeasurementQuery) -> Result<MeasurementIterator, ProviderError>;
}

pub struct MeasurementIterator(Box<dyn Iterator<Item = Result<Measurement, DomainError>> + 'static>);

impl MeasurementIterator {
    // Konstruktor für den Adapter
    pub fn new(iter: impl Iterator<Item = Result<Measurement, DomainError>> + 'static) -> Self {
        Self(Box::new(iter))
    }
}

impl Iterator for MeasurementIterator {
    type Item = Result<Measurement, DomainError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

pub struct MeasurementQuery {
    pub channel_id: ChannelId,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy)]
impl MeasurementQuery {
    pub fn for_channel(channel_id: ChannelId) -> Self {
        Self {
            channel_id,
            from: None,
            to: None,
        }
    }

    pub fn with_start(mut self, from: DateTime<Utc>) -> Self {
        self.from = Some(from);
        self
    }

    pub fn with_end(mut self, to: DateTime<Utc>) -> Self {
        self.to = Some(to);
        self
    }
}