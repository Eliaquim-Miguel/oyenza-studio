# Prayer times add-on

Prayer times is off by default. Switching it on in Settings downloads its
files; switching it off deletes them. The code that works out the times
(`crates/poni-core/src/prayer/`) is part of poni.exe and stays asleep until
the add-on is on.

This folder holds the add-on's files.

- `places.json`: the places people can pick. All 517 locations in Kemenag's
  prayer schedule (every kab/kota, plus five island points such as "Pulau
  Laut, Kab. Natuna"), named as Kemenag writes them. Each has the reference
  point and the Terbit and Maghrib horizon (`h0`) that reproduce Kemenag's
  own 2026 schedule with this engine, so the computed times land on
  Kemenag's minutes. Six Papua highland regencies carry Kemenag's wider
  twilight angles there (Subuh 21°, Isya 19°). Makkah, Madinah and Jeddah
  use Umm al-Qura. Settings keep the chosen place by its `id`, so never
  rename one.

## How the times are checked

`cargo test -p poni-core prayer` replays:

- Kemenag's published 2026 schedule for all 517 locations, 227,367 times:
  98.2% exact, never more than 1 minute off. Every place is at least 95%
  exact, and every miss sits within seconds of a minute boundary.
- Kanwil Kemenag Jawa Tengah's Ramadan 1447 H schedule for Kota Semarang,
  21 times, all exact.
- Oyen Umroh's prayer engine (adhan-js with the same rules) on 18,225
  place-days from 2026 to 2075, all identical to the minute.

The schedules came from api.myquran.com, which republishes Kemenag's
(bimasislam.kemenag.go.id). They are test data only: the add-on ships
places, never Kemenag's tables.
