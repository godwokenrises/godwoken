/*
 * Copyright 2022, The Cozo Project Authors. Licensed under MIT/Apache-2.0/BSD-3-Clause.
 */

#pragma once

#include <memory>
#include "rocksdb/utilities/transaction_db.h"
#include "rocksdb/utilities/options_util.h"

using namespace std;
using namespace rocksdb;

inline vector<ColumnFamilyDescriptor>
new_column_family_descriptor_vec(size_t len)
{
    vector<ColumnFamilyDescriptor> descriptors;
    descriptors.reserve(len + 1);
    for (size_t i = 0; i < len; i++)
    {
        descriptors.emplace_back(to_string(i), ColumnFamilyOptions());
    }
    descriptors.emplace_back("default", ColumnFamilyOptions());
    return descriptors;
}

TransactionDBOptions new_transaction_db_options()
{
    return TransactionDBOptions();
}

unique_ptr<WriteBatch> new_write_batch()
{
    return make_unique<WriteBatch>();
}

// Autocxx cannot access fields of non-pod type...
struct ReadOptionsWrapper : ReadOptions
{
    void set_snapshot(const Snapshot *snapshot_)
    {
        snapshot = snapshot_;
    }
};

struct DbOptionsWrapper
{
    string path;
    DBOptions db_options;
    vector<ColumnFamilyDescriptor> cf_descriptors;

    DbOptionsWrapper(string path_)
        : DbOptionsWrapper(path_, 0)
    {
    }

    DbOptionsWrapper(string path_, size_t columns)
        : path(path_), cf_descriptors(new_column_family_descriptor_vec(columns))
    {
    }

    DbOptionsWrapper(Slice path_, size_t columns)
        : path(path_.ToString()), cf_descriptors(new_column_family_descriptor_vec(columns))
    {
    }

    void set_create_if_missing(bool val)
    {
        db_options.create_if_missing = val;
    }

    void set_create_missing_column_families(bool val)
    {
        db_options.create_missing_column_families = val;
    }

    void set_compression(CompressionType comp)
    {
        for (ColumnFamilyDescriptor &x : cf_descriptors)
        {
            x.options.compression = comp;
        }
    }

    Status load(Slice options_file, size_t cache_size)
    {
        // Number of columns excluding the default.
        auto columns = cf_descriptors.size() - 1;
        auto cache = cache_size > 0 ? NewLRUCache(cache_size) : shared_ptr<Cache>();
        auto status = LoadOptionsFromFile(
            options_file.ToString(),
            Env::Default(),
            &db_options,
            &cf_descriptors,
            false,
            cache_size > 0 ? &cache : nullptr);
        if (!status.ok())
        {
            return status;
        }
        db_options.row_cache = cache;
        sort_and_complete_missing(columns);
        return status;
    }

    ColumnFamilyOptions *get_cf_option(size_t index)
    {
        return &cf_descriptors[index].options;
    }

    Status repair() const
    {
        return RepairDB(path, db_options, cf_descriptors);
    }

private:
    void sort_and_complete_missing(size_t columns)
    {
        unordered_map<string, ColumnFamilyDescriptor> cf_map;
        for (auto desc : cf_descriptors)
        {
            cf_map.emplace(desc.name, move(desc));
        }
        auto default_cf = cf_map["default"];
        cf_map.erase("default");

        cf_descriptors.clear();
        cf_descriptors.reserve(columns + 1);
        for (size_t i = 0; i < columns; i++)
        {
            auto name = to_string(i);
            auto it = cf_map.find(name);
            if (it != cf_map.end())
            {
                cf_descriptors.emplace_back(move(it->second));
            }
            else
            {
                cf_descriptors.emplace_back(name, default_cf.options);
            }
        }
        cf_descriptors.emplace_back(move(default_cf));
    }
};

struct TransactionWrapper;

// Note: make sure TransactionDBWrapper is Unpin.
struct TransactionDBWrapper
{
    unique_ptr<TransactionDB> db;
    std::vector<ColumnFamilyHandle *> cf_handles;

    Status open(
        const DbOptionsWrapper &options,
        const TransactionDBOptions &transaction_db_options)
    {
        TransactionDB *ptr;
        Status status = TransactionDB::Open(
            options.db_options,
            transaction_db_options,
            options.path,
            options.cf_descriptors,
            &cf_handles,
            &ptr);
        if (status.ok())
        {
            db.reset(ptr);
        }
        return status;
    }

    ~TransactionDBWrapper()
    {
        for (auto cf : cf_handles)
        {
            db->DestroyColumnFamilyHandle(cf);
        }
    }

    Status set_options(
        ColumnFamilyHandle *cf,
        Slice const *keys,
        Slice const *values,
        size_t len) const
    {
        auto options = unordered_map<string, string>();
        for (size_t i = 0; i < len; i++)
        {
            options[keys[i].ToString()] = values[i].ToString();
        }
        return db->SetOptions(cf, options);
    }

    Status set_db_options(
        Slice const *keys,
        Slice const *values,
        size_t len) const
    {
        auto options = unordered_map<string, string>();
        for (size_t i = 0; i < len; i++)
        {
            options[keys[i].ToString()] = values[i].ToString();
        }
        return db->SetDBOptions(options);
    }

    ColumnFamilyHandle *get_cf(size_t cf) const
    {
        if (cf >= cf_handles.size())
        {
            return nullptr;
        }
        return cf_handles[cf];
    }

    size_t default_col() const
    {
        return cf_handles.size() - 1;
    }

    Status clear_cf(size_t col)
    {
        auto cf = get_cf(col);
        if (!cf)
        {
            return Status::OK();
        }

        auto options = db->GetOptions(cf);

        Status status = db->DropColumnFamily(cf);
        if (!status.ok())
        {
            return status;
        }
        status = db->DestroyColumnFamilyHandle(cf);
        if (!status.ok())
        {
            return status;
        }
        status = db->CreateColumnFamily(options, to_string(col), &cf_handles[col]);
        return status;
    }

    Status drop_cf(size_t col)
    {
        auto cf = get_cf(col);
        if (!cf)
        {
            return Status::OK();
        }

        Status status = db->DropColumnFamily(cf);
        if (!status.ok())
        {
            return status;
        }
        status = db->DestroyColumnFamilyHandle(cf);
        if (!status.ok())
        {
            return status;
        }
        cf_handles[col] = nullptr;
        return status;
    }

    Status get(const ReadOptions &options, ColumnFamilyHandle *cf, const Slice &key, PinnableSlice *slice) const
    {
        return db->Get(options, cf, key, slice);
    }

    Status put(const WriteOptions &options, ColumnFamilyHandle *cf, const Slice &key, const Slice &value) const
    {
        return db->Put(options, cf, key, value);
    }

    Status del(const WriteOptions &options, ColumnFamilyHandle *cf, const Slice &key) const
    {
        return db->Delete(options, cf, key);
    }

    bool get_int_property(ColumnFamilyHandle *cf, const Slice &property, uint64_t *value) const
    {
        return db->GetIntProperty(cf, property, value);
    }

    unique_ptr<Iterator> iter(const ReadOptions &options, ColumnFamilyHandle *cf) const
    {
        return unique_ptr<Iterator>(db->NewIterator(options, cf));
    }

    TransactionWrapper begin(const WriteOptions &write_options, const TransactionOptions &transaction_options) const;

    Status write(const WriteOptions &wopts, const TransactionDBWriteOptimizations &opts, WriteBatch *updates) const
    {
        return db->Write(wopts, opts, updates);
    }

    const Snapshot *get_snapshot() const
    {
        return db->GetSnapshot();
    }

    void release_snapshot(const Snapshot *snapshot) const
    {
        db->ReleaseSnapshot(snapshot);
    }
};

// Note: make sure ReadOnlyDbWrapper is Unpin.
struct ReadOnlyDbWrapper
{
    unique_ptr<DB> db;
    std::vector<ColumnFamilyHandle *> cf_handles;

    Status open(
        const DbOptionsWrapper &options)
    {
        DB *ptr;
        Status status = DB::OpenForReadOnly(
            options.db_options,
            options.path,
            options.cf_descriptors,
            &cf_handles,
            &ptr);
        if (status.ok())
        {
            db.reset(ptr);
        }
        return status;
    }

    ~ReadOnlyDbWrapper()
    {
        for (auto cf : cf_handles)
        {
            db->DestroyColumnFamilyHandle(cf);
        }
    }

    ColumnFamilyHandle *get_cf(size_t cf) const
    {
        if (cf >= cf_handles.size())
        {
            return nullptr;
        }
        return cf_handles[cf];
    }

    size_t default_col() const
    {
        return cf_handles.size() - 1;
    }

    Status get(const ReadOptions &options, ColumnFamilyHandle *cf, const Slice &key, PinnableSlice *slice) const
    {
        return db->Get(options, cf, key, slice);
    }

    unique_ptr<Iterator> iter(const ReadOptions &options, ColumnFamilyHandle *cf) const
    {
        return unique_ptr<Iterator>(db->NewIterator(options, cf));
    }
};

// Note: make sure TransactionWrapper is Unpin.
struct TransactionWrapper
{
    unique_ptr<Transaction> tx;

    Status get(const ReadOptions &options, ColumnFamilyHandle *cf, const Slice &key, PinnableSlice *slice) const
    {
        return tx->Get(options, cf, key, slice);
    }

    Status put(ColumnFamilyHandle *cf, const Slice &key, const Slice &value)
    {
        return tx->Put(cf, key, value);
    }

    Status del(ColumnFamilyHandle *cf, const Slice &key)
    {
        return tx->Delete(cf, key);
    }

    const Snapshot *snapshot() const
    {
        return tx->GetSnapshot();
    }

    shared_ptr<Snapshot> timestamped_snapshot() const
    {
        // Because autocxx cannot handle shared_ptr<const Foo>.
        return const_pointer_cast<Snapshot>(tx->GetTimestampedSnapshot());
    }

    Status rollback()
    {
        return tx->Rollback();
    }

    Status commit()
    {
        return tx->Commit();
    }

    unique_ptr<Iterator> iter(const ReadOptions &options, ColumnFamilyHandle *cf) const
    {
        return unique_ptr<Iterator>(tx->GetIterator(options, cf));
    }
};

inline TransactionWrapper TransactionDBWrapper::begin(const WriteOptions &write_options, const TransactionOptions &transaction_options) const
{
    return {unique_ptr<Transaction>(db->BeginTransaction(write_options, transaction_options))};
}
