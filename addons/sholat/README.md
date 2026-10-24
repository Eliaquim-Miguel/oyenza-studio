# Prayer times add-on

Prayer times is off by default. Switching it on in Settings downloads its
files; switching it off deletes them. The code that works out the times
(`crates/poni-core/src/prayer/`) is part of poni.exe and stays asleep until
the add-on is on.

This folder holds the add-on's files.

- `places.json`: the places people can pick. Indonesian kab/kota are named
  as Kemenag writes them, each with the reference point and the Terbit and
  Maghrib horizon (`h0`) fitted to Kemenag's own 2026 schedule, so the
  computed times land on Kemenag's minutes. Makkah, Madinah and Jeddah use
  Umm al-Qura. Settings keep the chosen place by its `id`, so never rename
  one.

## How the times are checked

`cargo test -p poni-core prayer` replays:

- Kemenag's published 2026 schedule, 60,837 times: 97.6% exact, never more
  than 1 minute off. Every miss sits within seconds of a minute boundary.
- Kanwil Kemenag Jawa Tengah's Ramadan 1447 H schedule for Kota Semarang,
  21 times, all exact.
- Oyen Umroh's prayer engine (adhan-js with the same rules) on 13,455
  place-days from 2026 to 2075, all identical to the minute.

The schedules came from api.myquran.com, which republishes Kemenag's
(bimasislam.kemenag.go.id). They are test data only: the add-on ships
places, never Kemenag's tables.
