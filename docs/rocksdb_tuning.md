# Basic Rocksdb Tuning

Most of the time, the default config of rocksdb is fine. But we've seen a significant long time of delay(say 30sec maybe) when we try to restart a long-running godwoken process. Because rocksdb cannot flush data in the `memtable` to the disk before getting shutdown. So, for the next time, rocksdb will recover data through replay wal at the next time when it restarts. There are a lot to recover due to the default config.

See the recommended rocksdb options below:

```toml
## db.toml

[DBOptions]
bytes_per_sync=1048576
max_background_compactions=4
max_background_flushes=2
max_total_wal_size=134217728
keep_log_file_num=32

[CFOptions "default"]
level_compaction_dynamic_level_bytes=true
write_buffer_size=8388608
min_write_buffer_number_to_merge=1
max_write_buffer_number=2
max_write_buffer_size_to_maintain=-1

[CFOptions "18"]
prefix_extractor=8
level_compaction_dynamic_level_bytes=true
write_buffer_size=8388608
min_write_buffer_number_to_merge=1
max_write_buffer_number=2
max_write_buffer_size_to_maintain=-1

[CFOptions "20"]
prefix_extractor=32
level_compaction_dynamic_level_bytes=true
write_buffer_size=8388608
min_write_buffer_number_to_merge=1
max_write_buffer_number=2
max_write_buffer_size_to_maintain=-1

[TableOptions/BlockBasedTable "default"]
pin_l0_filter_and_index_blocks_in_cache=true
cache_index_and_filter_blocks=true
```

use db.toml in the godwoken config:
```toml
[store]
path = 'tuning_db/store.db'
options_file = 'db.toml'
cache_size = 268435456
```
