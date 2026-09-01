using FishNet.Documenting;
using FishNet.Managing;
using FishNet.Object.Synchronizing.Internal;
using FishNet.Serializing;
using GameKit.Dependencies.Utilities;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics.CodeAnalysis;

namespace FishNet.Object.Synchronizing
{
    [System.Serializable]
    public class SyncDictionary<TKey, TValue> : SyncBase, IDictionary<TKey, TValue>, IReadOnlyDictionary<TKey, TValue>
    {
        #region Types.
        /// <summary>
        /// Information needed to invoke a callback.
        /// </summary>
        private struct CachedOnChange
        {
            internal readonly SyncDictionaryOperation Operation;
            internal readonly TKey Key;
            internal readonly TValue Value;

            public CachedOnChange(SyncDictionaryOperation operation, TKey key, TValue value)
            {
                Operation = operation;
                Key = key;
                Value = value;
            }
        }

        /// <summary>
        /// Information about how the collection has changed.
        /// </summary>
        private struct ChangeData
        {
            internal readonly SyncDictionaryOperation Operation;
            internal readonly TKey Key;
            internal readonly TValue Value;
            internal readonly int CollectionCountAfterChange;

            public ChangeData(SyncDictionaryOperation operation, TKey key, TValue value, int collectionCountAfterChange)
            {
                Operation = operation;
                Key = key;
                Value = value;
                CollectionCountAfterChange = collectionCountAfterChange;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Implementation from Dictionary<TKey, TValue>. Not used.
        /// </summary>
        [APIExclude]
        public bool IsReadOnly => false;

        /// <summary>
        /// Delegate signature for when SyncDictionary changes.
        /// </summary>
        /// <param name = "op">Operation being completed, such as Add, Set, Remove.</param>
        /// <param name = "key">Key being modified.</param>
        /// <param name = "value">Value of operation.</param>
        /// <param name = "asServer">True if callback is on the server side. False is on the client side.</param>
        [APIExclude]
        public delegate void SyncDictionaryChanged(SyncDictionaryOperation op, TKey key, TValue value, bool asServer);

        /// <summary>
        /// Called when the SyncDictionary changes.
        /// </summary>
        public event SyncDictionaryChanged OnChange;
        /// <summary>
        /// Collection of objects.
        /// </summary>
        public Dictionary<TKey, TValue> Collection;
        /// <summary>
        /// Number of objects in the collection.
        /// </summary>
        public int Count => Collection.Count;
        /// <summary>
        /// Keys within the collection.
        /// </summary>
        public ICollection<TKey> Keys => Collection.Keys;
        [APIExclude]
        IEnumerable<TKey> IReadOnlyDictionary<TKey, TValue>.Keys => Collection.Keys;
        /// <summary>
        /// Values within the collection.
        /// </summary>
        public ICollection<TValue> Values => Collection.Values;
        [APIExclude]
        IEnumerable<TValue> IReadOnlyDictionary<TKey, TValue>.Values => Collection.Values;
        #endregion

        #region Private.
        /// <summary>
        /// Initial values for the dictionary.
        /// </summary>
        private Dictionary<TKey, TValue> _initialValues = new();
        /// <summary>
        /// Changed data which will be sent next tick.
        /// </summary>
        private List<ChangeData> _changed = new();
        /// <summary>
        /// Server OnChange events waiting for start callbacks.
        /// </summary>
        private List<CachedOnChange> _serverOnChanges = new();
        /// <summary>
        /// Client OnChange events waiting for start callbacks.
        /// </summary>
        private List<CachedOnChange> _clientOnChanges = new();
        /// <summary>
        /// True if values have changed since initialization.
        /// The only reasonable way to reset this during a Reset call is by duplicating the original list and setting all values to it on reset.
        /// </summary>
        private bool _valuesChanged;
        /// <summary>
        /// True to send all values in the next WriteDelta.
        /// </summary>
        private bool _sendAll;
        #endregion

        #region Constructors.
        public SyncDictionary(SyncTypeSettings settings = new()) : this(CollectionCaches<TKey, TValue>.RetrieveDictionary(), settings) { }

        public SyncDictionary(Dictionary<TKey, TValue> collection, SyncTypeSettings settings = new()) : base(settings)
        {
            Collection = collection == null ? CollectionCaches<TKey, TValue>.RetrieveDictionary() : collection;
            _initialValues = CollectionCaches<TKey, TValue>.RetrieveDictionary();
            _changed = CollectionCaches<ChangeData>.RetrieveList();
            _serverOnChanges = CollectionCaches<CachedOnChange>.RetrieveList();
            _clientOnChanges = CollectionCaches<CachedOnChange>.RetrieveList();
        }
        #endregion

        #region Deconstructor.
        ~SyncDictionary()
        {
            CollectionCaches<TKey, TValue>.StoreAndDefault(ref Collection);
            CollectionCaches<TKey, TValue>.StoreAndDefault(ref _initialValues);
            CollectionCaches<ChangeData>.StoreAndDefault(ref _changed);
            CollectionCaches<CachedOnChange>.StoreAndDefault(ref _serverOnChanges);
            CollectionCaches<CachedOnChange>.StoreAndDefault(ref _clientOnChanges);
        }
        #endregion

        /// <summary>
        /// Gets the collection being used within this SyncList.
        /// </summary>
        /// <param name = "asServer">True if returning the server value, false if client value. The values will only differ when running as host. While asServer is true the most current values on server will be returned, and while false the latest values received by client will be returned.</param>
        /// <returns>The used collection.</returns>
        public Dictionary<TKey, TValue> GetCollection(bool asServer)
        {
            return Collection;
        }

        /// <summary>
        /// Called when the SyncType has been registered, but not yet initialized over the network.
        /// </summary>
        protected override void Initialized()
        {
            base.Initialized();

            // Initialize collections if needed. OdinInspector can cause them to become deinitialized.
#if ODIN_INSPECTOR
            if (_initialValues == null)
                _initialValues = new();
            if (_changed == null)
                _changed = new();
            if (_serverOnChanges == null)
                _serverOnChanges = new();
            if (_clientOnChanges == null)
                _clientOnChanges = new();
#endif

            foreach (KeyValuePair<TKey, TValue> item in Collection)
                _initialValues[item.Key] = item.Value;
        }

        /// <summary>
        /// Adds an operation and invokes callback locally.
        /// Internal use.
        /// May be used for custom SyncObjects.
        /// </summary>
        /// <param name = "operation"></param>
        /// <param name = "key"></param>
        /// <param name = "value"></param>
        [APIExclude]
        private void AddOperation(SyncDictionaryOperation operation, TKey key, TValue value, int collectionCountAfterChange)
        {
            if (!IsInitialized)
                return;

            /* asServer might be true if the client is setting the value
             * through user code. Typically synctypes can only be set
             * by the server, that's why it is assumed asServer via user code.
             * However, when excluding owner for the synctype the client should
             * have permission to update the value locally for use with
             * prediction. */
            bool asServerInvoke = !IsNetworkInitialized || NetworkBehaviour.IsServerStarted;

            if (asServerInvoke)
            {
                _valuesChanged = true;
                if (base.Dirty())
                {
                    ChangeData change = new(operation, key, value, collectionCountAfterChange);
                    _changed.Add(change);
                }
            }

            InvokeOnChange(operation, key, value, asServerInvoke);
        }

        /// <summary>
        /// Called after OnStartXXXX has occurred.
        /// </summary>
        /// <param name = "asServer">True if OnStartServer was called, false if OnStartClient.</param>
        protected internal override void OnStartCallback(bool asServer)
        {
            base.OnStartCallback(asServer);
            List<CachedOnChange> collection = asServer ? _serverOnChanges : _clientOnChanges;

            if (OnChange != null)
            {
                foreach (CachedOnChange item in collection)
                    OnChange.Invoke(item.Operation, item.Key, item.Value, asServer);
            }

            collection.Clear();
        }

        /// <summary>
        /// Writes an operation and data required by all operations.
        /// </summary>
        private void WriteOperationHeader(PooledWriter writer, SyncDictionaryOperation operation, int collectionCountAfterChange)
        {
            writer.WriteUInt8Unpacked((byte)operation);
            writer.WriteInt32(collectionCountAfterChange);
        }

        /// <summary>
        /// Reads an operation and data required by all operations.
        /// </summary>
        private void ReadOperationHeader(PooledReader reader, out SyncDictionaryOperation operation, out int collectionCountAfterChange)
        {
            operation = (SyncDictionaryOperation)reader.ReadUInt8Unpacked();
            collectionCountAfterChange = reader.ReadInt32();
        }

        /// <summary>
        /// Writes all changed values.
        /// Internal use.
        /// May be used for custom SyncObjects.
        /// </summary>
        /// <param name = "writer"></param>
        /// <param name = "resetSyncTick">True to set the next time data may sync.</param>
        [APIExclude]
        protected internal override void WriteDelta(PooledWriter writer, bool resetSyncTick = true)
        {
            // If sending all then clear changed and write full.
            if (_sendAll)
            {
                _sendAll = false;
                _changed.Clear();
                WriteFull(writer);
            }
            else
            {
                base.WriteDelta(writer, resetSyncTick);

                // False for not full write.
                writer.WriteBoolean(false);

                writer.WriteInt32(_changed.Count);

                for (int i = 0; i < _changed.Count; i++)
                {
                    ChangeData change = _changed[i];

                    WriteOperationHeader(writer, change.Operation, change.CollectionCountAfterChange);

                    // Clear does not need to write anymore data so it is not included in checks.
                    if (change.Operation == SyncDictionaryOperation.Add || change.Operation == SyncDictionaryOperation.Set)
                    {
                        writer.Write(change.Key);
                        writer.Write(change.Value);
                    }
                    else if (change.Operation == SyncDictionaryOperation.Remove)
                    {
                        writer.Write(change.Key);
                    }
                }

                _changed.Clear();
            }
        }

        /// <summary>
        /// Writers all values if not initial values.
        /// Internal use.
        /// May be used for custom SyncObjects.
        /// </summary>
        /// <param name = "writer"></param>
        [APIExclude]
        protected internal override void WriteFull(PooledWriter writer)
        {
            if (!_valuesChanged)
                return;

            base.WriteHeader(writer, false);

            // True for full write.
            writer.WriteBoolean(true);

            writer.WriteInt32(Collection.Count);

            int iteration = 0;
            foreach (KeyValuePair<TKey, TValue> item in Collection)
            {
                WriteOperationHeader(writer, SyncDictionaryOperation.Add, iteration + 1);
                writer.Write(item.Key);
                writer.Write(item.Value);

                iteration++;
            }
        }

        /// <summary>
        /// Reads and sets the current values for server or client.
        /// </summary>
        [APIExclude]
        protected internal override void Read(PooledReader reader, bool asServer)
        {
            SetReadArguments(reader, asServer, out bool newChangeId, out bool asClientHost, out bool canModifyValues);

            // True to warn if this object was deinitialized on the server.
            bool deinitialized = asClientHost && !OnStartServerCalled;
            if (deinitialized)
                NetworkManager.LogWarning($"SyncType {GetType().Name} received a Read but was deinitialized on the server. Client callback values may be incorrect. This is a ClientHost limitation.");

            IDictionary<TKey, TValue> collection = Collection;

            bool fullWrite = reader.ReadBoolean();

            // Clear collection since it's a full write.
            if (canModifyValues && fullWrite)
                collection.Clear();

            int changes = reader.ReadInt32();
            for (int i = 0; i < changes; i++)
            {
                ReadOperationHeader(reader, out SyncDictionaryOperation operation, out int collectionCountAfterChange);

                TKey key = default;
                TValue value = default;

                /* Add, Set.
                 * Use the Set code for add and set,
                 * especially so collection doesn't throw
                 * if entry has already been added. */
                if (operation == SyncDictionaryOperation.Add || operation == SyncDictionaryOperation.Set)
                {
                    /* If a set then the collection count should remain the same.
                     * Otherwise, the count should increase by 1. */
                    int sizeExpectedAfterChange = operation == SyncDictionaryOperation.Add ? collection.Count + 1 : collection.Count;

                    key = reader.Read<TKey>();
                    value = reader.Read<TValue>();

                    if (canModifyValues)
                    {
                        // Integrity validation.
                        if (sizeExpectedAfterChange == collectionCountAfterChange)
                            collection[key] = value;
                    }
                }
                // Clear.
                else if (operation == SyncDictionaryOperation.Clear)
                {
                    if (canModifyValues)
                    {
                        // No integrity validation needed. 
                        collection.Clear();
                    }
                }
                //Remove.
                else if (operation == SyncDictionaryOperation.Remove)
                {
                    key = reader.Read<TKey>();

                    if (canModifyValues)
                    {
                        //Integrity validation.
                        if (collection.Count - 1 == collectionCountAfterChange)
                            collection.Remove(key);
                    }
                }

                if (newChangeId)
                    InvokeOnChange(operation, key, value, false);
            }

            //If changes were made invoke complete after all have been read.
            if (newChangeId && changes > 0)
                InvokeOnChange(SyncDictionaryOperation.Complete, default, default, false);
        }

        /// <summary>
        /// Invokes OnChanged callback.
        /// </summary>
        private void InvokeOnChange(SyncDictionaryOperation operation, TKey key, TValue value, bool asServer)
        {
            if (asServer)
            {
                if (NetworkBehaviour.OnStartServerCalled)
                    OnChange?.Invoke(operation, key, value, asServer);
                else
                    _serverOnChanges.Add(new(operation, key, value));
            }
            else
            {
                if (NetworkBehaviour.OnStartClientCalled)
                    OnChange?.Invoke(operation, key, value, asServer);
                else
                    _clientOnChanges.Add(new(operation, key, value));
            }
        }

        /// <summary>
        /// Resets to initialized values.
        /// </summary>
        [APIExclude]
        protected internal override void ResetState(bool asServer)
        {
            base.ResetState(asServer);

            if (CanReset(asServer))
            {
                _sendAll = false;
                _changed.Clear();
                Collection.Clear();
                _valuesChanged = false;

                foreach (KeyValuePair<TKey, TValue> item in _initialValues)
                    Collection[item.Key] = item.Value;
            }
        }

        /// <summary>
        /// Adds item.
        /// </summary>
        /// <param name = "item">Item to add.</param>
        public void Add(KeyValuePair<TKey, TValue> item)
        {
            Add(item.Key, item.Value);
        }

        /// <summary>
        /// Adds key and value.
        /// </summary>
        /// <param name = "key">Key to add.</param>
        /// <param name = "value">Value for key.</param>
        public void Add(TKey key, TValue value)
        {
            Add(key, value, true);
        }

        private void Add(TKey key, TValue value, bool asServer)
        {
            if (!CanNetworkSetValues(true))
                return;

            Collection.Add(key, value);
            /* We can perform add operation without checks, as Add would have failed above
             * if entry already existed. */
            if (asServer)
                AddOperation(SyncDictionaryOperation.Add, key, value, Collection.Count);
        }

        /// <summary>
        /// Clears all values.
        /// </summary>
        public void Clear()
        {
            Clear(true);
        }

        private void Clear(bool asServer)
        {
            if (!CanNetworkSetValues(true))
                return;

            Collection.Clear();
            if (asServer)
                AddOperation(SyncDictionaryOperation.Clear, default, default, Collection.Count);
        }

        /// <summary>
        /// Returns if key exist.
        /// </summary>
        /// <param name = "key">Key to use.</param>
        /// <returns>True if found.</returns>
        public bool ContainsKey(TKey key)
        {
            return Collection.ContainsKey(key);
        }

        /// <summary>
        /// Returns if item exist.
        /// </summary>
        /// <param name = "item">Item to use.</param>
        /// <returns>True if found.</returns>
        public bool Contains(KeyValuePair<TKey, TValue> item)
        {
            return TryGetValue(item.Key, out TValue value) && EqualityComparer<TValue>.Default.Equals(value, item.Value);
        }

        /// <summary>
        /// Copies collection to an array.
        /// </summary>
        /// <param name = "array">Array to copy to.</param>
        /// <param name = "offset">Offset of array data is copied to.</param>
        public void CopyTo([NotNull] KeyValuePair<TKey, TValue>[] array, int offset)
        {
            if (offset <= -1 || offset >= array.Length)
            {
                NetworkManager.LogError($"Index is out of range.");
                return;
            }

            int remaining = array.Length - offset;
            if (remaining < Count)
            {
                NetworkManager.LogError($"Array is not large enough to copy data. Array is of length {array.Length}, index is {offset}, and number of values to be copied is {Count.ToString()}.");
                return;
            }

            int i = offset;
            foreach (KeyValuePair<TKey, TValue> item in Collection)
            {
                array[i] = item;
                i++;
            }
        }

        /// <summary>
        /// Removes a key.
        /// </summary>
        /// <param name = "key">Key to remove.</param>
        /// <returns>True if removed.</returns>
        public bool Remove(TKey key)
        {
            if (!CanNetworkSetValues(true))
                return false;

            if (Collection.Remove(key))
            {
                AddOperation(SyncDictionaryOperation.Remove, key, default, Collection.Count);
                return true;
            }

            return false;
        }

        /// <summary>
        /// Removes an item.
        /// </summary>
        /// <param name = "item">Item to remove.</param>
        /// <returns>True if removed.</returns>
        public bool Remove(KeyValuePair<TKey, TValue> item)
        {
            return Remove(item.Key);
        }

        /// <summary>
        /// Tries to get value from key.
        /// </summary>
        /// <param name = "key">Key to use.</param>
        /// <param name = "value">Variable to output to.</param>
        /// <returns>True if able to output value.</returns>
        public bool TryGetValue(TKey key, out TValue value)
        {
            return Collection.TryGetValueIL2CPP(key, out value);
        }

        /// <summary>
        /// Gets or sets value for a key.
        /// </summary>
        /// <param name = "key">Key to use.</param>
        /// <returns>Value when using as Get.</returns>
        public TValue this[TKey key]
        {
            get => Collection[key];
            set
            {
                if (!CanNetworkSetValues(true))
                    return;

                /* Change to Add if entry does not exist yet. */
                SyncDictionaryOperation operation = Collection.ContainsKey(key) ? SyncDictionaryOperation.Set : SyncDictionaryOperation.Add;

                Collection[key] = value;

                AddOperation(operation, key, value, Collection.Count);
            }
        }

        /// <summary>
        /// Dirties the entire collection forcing a full send.
        /// </summary>
        public void DirtyAll()
        {
            if (!IsInitialized)
                return;
            if (!CanNetworkSetValues(log: true))
                return;

            if (base.Dirty())
                _sendAll = true;
        }

        /// <summary>
        /// Dirties an entry by key.
        /// </summary>
        /// <param name = "key">Key to dirty.</param>
        public void Dirty(TKey key)
        {
            if (!IsInitialized)
                return;
            if (!CanNetworkSetValues(true))
                return;

            if (Collection.TryGetValueIL2CPP(key, out TValue value))
                AddOperation(SyncDictionaryOperation.Set, key, value, Collection.Count);
        }

        /// <summary>
        /// Dirties an entry by value.
        /// This operation can be very expensive, will cause allocations, and may fail if your value cannot be compared.
        /// </summary>
        /// <param name = "value">Value to dirty.</param>
        /// <returns>True if value was found and marked dirty.</returns>
        public bool Dirty(TValue value, EqualityComparer<TValue> comparer = null)
        {
            if (!IsInitialized)
                return false;
            if (!CanNetworkSetValues(true))
                return false;

            if (comparer == null)
                comparer = EqualityComparer<TValue>.Default;

            foreach (KeyValuePair<TKey, TValue> item in Collection)
            {
                if (comparer.Equals(item.Value, value))
                {
                    AddOperation(SyncDictionaryOperation.Set, item.Key, value, Collection.Count);
                    return true;
                }
            }

            //Not found.
            return false;
        }

        /// <summary>
        /// Gets the IEnumerator for the collection.
        /// </summary>
        /// <returns></returns>
        public IEnumerator<KeyValuePair<TKey, TValue>> GetEnumerator() => Collection.GetEnumerator();

        /// <summary>
        /// Gets the IEnumerator for the collection.
        /// </summary>
        /// <returns></returns>
        IEnumerator IEnumerable.GetEnumerator() => Collection.GetEnumerator();
    }
}