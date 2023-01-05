/*
 * This file is a self-contained example of writing to a Delta table using the Arrow
 * `RecordBatch` API rather than pushing the data through a JSON intermediary
 *
 */

use chrono::prelude::*;
use deltalake::action::*;
use deltalake::arrow::array::*;
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::writer::{DeltaWriter, RecordBatchWriter};
use deltalake::*;
use log::*;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/*
 * The main function gets everything started, but does not contain any meaningful
 * example code for writing to Delta tables
 */
#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Only enabling pretty env logger for debug builds
    #[cfg(debug_assertions)]
    pretty_env_logger::init();
    info!("Logger initialized");

    let table_uri = std::env::var("TABLE_URI")?;
    info!("Using the location of: {:?}", table_uri);

    let table_path = Path::new(&table_uri);

    let mut table = match Path::join(table_path, "_delta_log").is_dir() {
        true => {
            /* The table has been created already */
            info!("Opening the table for writing");
            deltalake::open_table(
                table_path
                    .to_str()
                    .expect("Could not convert table path to a str"),
            )
            .await?
        }
        false => {
            /* The table directory has not been initialized as a Delta table */
            info!("It doesn't look like our delta table has been created");
            create_initialized_table(&table_path).await
        }
    };

    let mut writer =
        RecordBatchWriter::for_table(&table).expect("Failed to make RecordBatchWriter");

    let records = fetch_readings();
    let batch = convert_to_batch(&writer, &records);

    writer.write(batch).await?;

    let adds = writer
        .flush_and_commit(&mut table)
        .await
        .expect("Failed to flush write");
    info!("{} adds written", adds);

    Ok(())
}

/*
 * Pilfered from writer/test_utils.rs in delta-rs
 */
async fn create_initialized_table(table_path: &Path) -> DeltaTable {
    let mut table = DeltaTableBuilder::from_uri(table_path.to_str().unwrap())
        .build()
        .unwrap();
    let table_schema = WeatherRecord::schema();
    let mut commit_info = serde_json::Map::<String, serde_json::Value>::new();
    commit_info.insert(
        "operation".to_string(),
        serde_json::Value::String("CREATE TABLE".to_string()),
    );
    commit_info.insert(
        "userName".to_string(),
        serde_json::Value::String("test user".to_string()),
    );

    let protocol = Protocol {
        min_reader_version: 1,
        min_writer_version: 1,
    };

    let metadata = DeltaTableMetaData::new(None, None, None, table_schema, vec![], HashMap::new());

    table
        .create(metadata, protocol, Some(commit_info), None)
        .await
        .unwrap();

    table
}

// Creating a simple type alias for improved readability
type Fahrenheit = i32;

/*
 * WeatherRecord is just a simple example structure to represent a row in the
 * delta table. Imagine a time-series of weather data which is being recorded
 * by a small sensor.
 */
struct WeatherRecord {
    timestamp: DateTime<Utc>,
    temp: Fahrenheit,
    lat: f64,
    long: f64,
}

impl WeatherRecord {
    fn schema() -> Schema {
        Schema::new(vec![
            SchemaField::new(
                "timestamp".to_string(),
                SchemaDataType::primitive("timestamp".to_string()),
                true,
                HashMap::new(),
            ),
            SchemaField::new(
                "temp".to_string(),
                SchemaDataType::primitive("integer".to_string()),
                true,
                HashMap::new(),
            ),
            SchemaField::new(
                "lat".to_string(),
                SchemaDataType::primitive("double".to_string()),
                true,
                HashMap::new(),
            ),
            SchemaField::new(
                "long".to_string(),
                SchemaDataType::primitive("double".to_string()),
                true,
                HashMap::new(),
            ),
        ])
    }
}

impl Default for WeatherRecord {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            temp: 72,
            lat: 39.61940984546992,
            long: -119.22916208856955,
        }
    }
}

/*
 * This function just generates a series of 5 temperature readings to be written
 * to the table
 */
fn fetch_readings() -> Vec<WeatherRecord> {
    let mut readings = vec![];

    for i in 1..=5 {
        let mut wx = WeatherRecord::default();
        wx.temp = wx.temp - i;
        readings.push(wx);
    }
    readings
}

/*
 * The convert to batch function does some of the heavy lifting for writing a
 * `RecordBatch` to a delta table. In essence, the Vec of WeatherRecord needs to
 * turned into a columnar format in order to be written correctly.
 *
 * That is to say that the following example rows:
 *  | ts | temp | lat | long |
 *  | 0  | 72   | 0.0 | 0.0  |
 *  | 1  | 71   | 0.0 | 0.0  |
 *  | 2  | 78   | 0.0 | 0.0  |
 *
 *  Must be converted into a data structure where all timestamps are together,
 *  ```
 *  let ts = vec![0, 1, 2];
 *  let temp = vec![72, 71, 78];
 *  ```
 *
 *  The Arrow Rust array primitives are _very_ fickle and so creating a direct
 *  transformation is quite tricky in Rust, whereas in Python or another loosely
 *  typed language it might be simpler.
 */
fn convert_to_batch(writer: &RecordBatchWriter, records: &Vec<WeatherRecord>) -> RecordBatch {
    let mut ts = vec![];
    let mut temp = vec![];
    let mut lat = vec![];
    let mut long = vec![];

    for record in records {
        ts.push(record.timestamp.timestamp_micros());
        temp.push(record.temp);
        lat.push(record.lat);
        long.push(record.long);
    }

    let arrow_array: Vec<Arc<dyn Array>> = vec![
        Arc::new(TimestampMicrosecondArray::from(ts)),
        Arc::new(Int32Array::from(temp)),
        Arc::new(Float64Array::from(lat)),
        Arc::new(Float64Array::from(long)),
    ];

    RecordBatch::try_new(writer.arrow_schema(), arrow_array).expect("Failed to create RecordBatch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_readings() {
        let readings = fetch_readings();
        assert_eq!(
            5,
            readings.len(),
            "fetch_readings() should return 5 readings"
        );
    }

    #[test]
    fn test_schema() {
        let schema: Schema = WeatherRecord::schema();
        assert_eq!(schema.get_fields().len(), 4, "schema should have 4 fields");
    }
}
