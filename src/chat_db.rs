use chrono;
use rusqlite;

use crate::error::Error;

type DateTime = chrono::DateTime<chrono::Utc>;

pub static DB_FILENAME: &str = "chat.db";

/// A wa message.
pub struct WaEntry
{
    /// The messages ID that the wa is for.
    pub wa_to: i64,
    /// The ID of the wa message.
    pub id: i64,
    /// The user ID that sends the wa.
    pub waer: i64,
    /// The display name of the waer. This could be a nick name or a full name.
    pub waer_name: String,
    /// The time of the wa message.
    pub time: DateTime,
}

fn connect() -> Result<rusqlite::Connection, Error>
{
    rusqlite::Connection::open(DB_FILENAME).map_err(
        |_| error!(DBError, "Failed to open/create chat database"))
}

pub fn initialize() -> Result<(), Error>
{
    let conn = connect()?;
    conn.execute(
        "CREATE TABLE was (
                  id              INTEGER PRIMARY KEY,
                  wa_to           INTEGER,
                  waer            INTEGER,
                  waer_name       TEXT NOT NULL,
                  time            INTEGER
                  );",
        rusqlite::NO_PARAMS,
    ).map_err(|_| error!(DBError, "Failed to create table 'was'"))?;
    Ok(())
}

/// Add a wa message to the database. Return the number of wa-s for
/// the message that `wa` is for.
pub fn addWa(wa: WaEntry) -> Result<u32, Error>
{
    let conn = connect()?;

    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM was WHERE wa_to = ?1;",
        rusqlite::params![wa.wa_to], |row| row.get(0))
        .map_err(|_| error!(DBError, "Failed to get count of was"))?;

    conn.execute(
        "INSERT INTO was (id, wa_to, waer, waer_name, time)
         VALUES (?1, ?2, ?3, ?4, ?5);",
        rusqlite::params![wa.id, wa.wa_to, wa.waer, wa.waer_name,
                          wa.time.timestamp()])
        .map_err(|_| error!(DBError, "Failed to add a wa"))?;
    Ok(count + 1)
}

/// Who did the most wa-s in the last `time_period`? Return the name
/// of the waer and the number of wa-s from this waer.
pub fn bestWaer(time_period: chrono::Duration) -> Result<(String, u32), Error>
{
    let now = chrono::offset::Utc::now();
    let conn = connect()?;
    let row = conn.query_row(
        "SELECT waer_name, COUNT(*) as count FROM was WHERE time > ?1
         GROUP BY waer ORDER BY count DESC LIMIT 1;",
        rusqlite::params![(now - time_period).timestamp()],
        |row| Ok((row.get(0), row.get(1))))
        .map_err(|_| error!(DBError, "Failed to get best waer"))?;

    Ok((row.0.map_err(|_| error!(DBError, "Failed to get waer_name"))?,
        row.1.map_err(|_| error!(DBError, "Failed to get wa count"))?))
}

/// Which msg is the most wa-ed during the last `time_period`? Return
/// msg ID and number of wa-s.
pub fn bestWaable(time_period: chrono::Duration) -> Result<(i64, u32), Error>
{
    let now = chrono::offset::Utc::now();
    let conn = connect()?;
    let row = conn.query_row(
        "SELECT wa_to, COUNT(*) as count FROM was WHERE time > ?1
         GROUP BY wa_to ORDER BY count DESC LIMIT 1;",
        rusqlite::params![(now - time_period).timestamp()],
        |row| Ok((row.get(0), row.get(1))))
        .map_err(|_| error!(DBError, "Failed to get best waable"))?;

    Ok((row.0.map_err(|_| error!(DBError, "Failed to get wa_to"))?,
        row.1.map_err(|_| error!(DBError, "Failed to get wa count"))?))
}
