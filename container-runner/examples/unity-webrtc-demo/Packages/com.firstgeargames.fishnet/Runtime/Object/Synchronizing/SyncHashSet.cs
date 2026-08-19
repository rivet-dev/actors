using FishNet.Documenting;
using FishNet.Managing;
using FishNet.Object.Synchronizing.Internal;
using FishNet.Serializing;
using GameKit.Dependencies.Utilities;
using System.Collections;
using System.Collections.Generic;

namespace FishNet.Object.Synchronizing
{
    [System.Serializable]
    public class SyncHashSet<T> : SyncBase, ISet<T>
    {
        #region Types.
        /// <summary>
        /// Information needed to invoke a callback.
        /// </summary>
        private struct CachedOnChange
        {
            internal readonly SyncHashSetOperation Operation;
            internal readonly T Item;

            public CachedOnChange(SyncHashSetOperation operation, T item)
            {
                Operation = operation;
                Item = item;
            }
        }

        /// <summary>
        /// Information about how the collection has changed.
        /// </summary>
        private struct ChangeData
        {
            internal readonly SyncHashSetOperation Operation;
            internal readonly T Item;
            internal readonly int CollectionCountAfterChange;

            public ChangeData(SyncHashSetOperation operation, T item, int collectionCountAfterChange)
            {
                Operation = operation;
                Item = item;
                CollectionCountAfterChange = collectionCountAfterChange;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Implementation from List<T>. Not used.
        /// </summary>
        [APIExclude]
        public bool IsReadOnly => false;

        /// <summary>
        /// Delegate signature for when SyncList changes.
        /// </summary>
        /// <param name = "op">Type of change.</param>
        /// <param name = "item">Item which was modified.</param>
        /// <param name = "asServer">True if callback is occuring on the server.</param>
        [APIExclude]
        public delegate void SyncHashSetChanged(SyncHashSetOperation op, T item, bool asServer);

        /// <summary>
        /// Called when the SyncList changes.
        /// </summary>
        public event SyncHashSetChanged OnChange;
        /// <summary>
        /// Collection of objects.
        /// </summary>
        public HashSet<T> Collection;
        /// <summary>
        /// Number of objects in the collection.
        /// </summary>
        public int Count => Collection.Count;
        #endregion

        #region Private.
        /// <summary>
        /// ListCache for comparing.
        /// </summary>
        private static List<T> _cache = new();
        /// <summary>
        /// Values upon initialization.
        /// </summary>
        private HashSet<T> _initialValues;
        /// <summary>
        /// Changed data which will be sent next tick.
        /// </summary>
        private List<ChangeData> _changed;
        /// <summary>
        /// Server OnChange events waiting for start callbacks.
        /// </summary>
        private List<CachedOnChange> _serverOnChanges;
        /// <summary>
        /// Client OnChange events waiting for start callbacks.
        /// </summary>
        private List<CachedOnChange> _clientOnChanges;
        /// <summary>
        /// Comparer to see if entries change when calling public methods.
        /// // Not used right now.
        /// </summary>
        private readonly IEqualityComparer<T> _comparer;
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
        public SyncHashSet(SyncTypeSettings settings = new()) : this(CollectionCaches<T>.RetrieveHashSet(), EqualityComparer<T>.Default, settings) { }
        public SyncHashSet(IEqualityComparer<T> comparer, SyncTypeSettings settings = new()) : this(CollectionCaches<T>.RetrieveHashSet(), comparer == null ? EqualityComparer<T>.Default : comparer, settings) { }

        public SyncHashSet(HashSet<T> collection, IEqualityComparer<T> comparer = null, SyncTypeSettings settings = new()) : base(settings)
        {
            _comparer = comparer == null ? EqualityComparer<T>.Default : comparer;
            Collection = collection == null ? CollectionCaches<T>.RetrieveHashSet() : collection;

            _initialValues = CollectionCaches<T>.RetrieveHashSet();
            _changed = CollectionCaches<ChangeData>.RetrieveList();
            _serverOnChanges = CollectionCaches<CachedOnChange>.RetrieveList();
            _clientOnChanges = CollectionCaches<CachedOnChange>.RetrieveList();
        }
        #endregion

        #region Deconstructor.
        ~SyncHashSet()
        {
            CollectionCaches<T>.StoreAndDefault(ref Collection);
            CollectionCaches<T>.StoreAndDefault(ref _initialValues);
            CollectionCaches<ChangeData>.StoreAndDefault(ref _changed);
            CollectionCaches<CachedOnChange>.StoreAndDefault(ref _serverOnChanges);
            CollectionCaches<CachedOnChange>.StoreAndDefault(ref _clientOnChanges);
        }
        #endregion

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
            foreach (T item in Collection)
                _initialValues.Add(item);
        }

        /// <summary>
        /// Gets the collection being used within this SyncList.
        /// </summary>
        /// <returns></returns>
        public HashSet<T> GetCollection(bool asServer)
        {
            return Collection;
        }

        /// <summary>
        /// Adds an operation and invokes locally.
        /// </summary>
        private void AddOperation(SyncHashSetOperation operation, T item, int collectionCountAfterChange)
        {
            if (!IsInitialized)
                return;

            bool asServerInvoke = !IsNetworkInitialized || NetworkBehaviour.IsServerStarted;

            if (asServerInvoke)
            {
                _valuesChanged = true;
                if (base.Dirty())
                {
                    ChangeData change = new(operation, item, collectionCountAfterChange);
                    _changed.Add(change);
                }
            }

            InvokeOnChange(operation, item, asServerInvoke);
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
                    OnChange.Invoke(item.Operation, item.Item, asServer);
            }

            collection.Clear();
        }

        /// <summary>
        /// Writes an operation and data required by all operations.
        /// </summary>
        private void WriteOperationHeader(PooledWriter writer, SyncHashSetOperation operation, int collectionCountAfterChange)
        {
            writer.WriteUInt8Unpacked((byte)operation);
            writer.WriteInt32(collectionCountAfterChange);
        }

        /// <summary>
        /// Reads an operation and data required by all operations.
        /// </summary>
        private void ReadOperationHeader(PooledReader reader, out SyncHashSetOperation operation, out int collectionCountAfterChange)
        {
            operation = (SyncHashSetOperation)reader.ReadUInt8Unpacked();
            collectionCountAfterChange = reader.ReadInt32();
        }

        /// <summary>
        /// Writes all changed values.
        /// </summary>
        /// <param name = "writer"></param>
        /// <param name = "resetSyncTick">True to set the next time data may sync.</param>
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
                    if (change.Operation == SyncHashSetOperation.Add || change.Operation == SyncHashSetOperation.Remove || change.Operation == SyncHashSetOperation.Set)
                        writer.Write(change.Item);
                }

                _changed.Clear();
            }
        }

        /// <summary>
        /// Writes all values if not initial values.
        /// </summary>
        /// <param name = "writer"></param>
        protected internal override void WriteFull(PooledWriter writer)
        {
            if (!_valuesChanged)
                return;

            base.WriteHeader(writer, false);
            // True for full write.
            writer.WriteBoolean(true);

            int count = Collection.Count;
            writer.WriteInt32(count);

            int iteration = 0;
            foreach (T item in Collection)
            {
                WriteOperationHeader(writer, SyncHashSetOperation.Add, collectionCountAfterChange: iteration + 1);
                writer.Write(item);

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

            ISet<T> collection = Collection;

            bool fullWrite = reader.ReadBoolean();

            // Clear collection since it's a full write.
            if (canModifyValues && fullWrite)
                collection.Clear();

            int changes = reader.ReadInt32();
            for (int i = 0; i < changes; i++)
            {
                ReadOperationHeader(reader, out SyncHashSetOperation operation, out int collectionCountAfterChange);

                T next = default;

                // Add.
                if (operation == SyncHashSetOperation.Add)
                {
                    next = reader.Read<T>();

                    if (canModifyValues)
                    {
                        // Integrity validation.
                        if (collection.Count + 1 == collectionCountAfterChange)
                            collection.Add(next);
                    }
                }
                // Clear.
                else if (operation == SyncHashSetOperation.Clear)
                {
                    if (canModifyValues)
                    {
                        // No integrity validation needed. 
                        collection.Clear();
                    }
                }
                // Remove.
                else if (operation == SyncHashSetOperation.Remove)
                {
                    next = reader.Read<T>();

                    if (canModifyValues)
                    {
                        // Integrity validation.
                        if (collection.Count - 1 == collectionCountAfterChange)
                            collection.Remove(next);
                    }
                }
                // Set.
                else if (operation == SyncHashSetOperation.Set)
                {
                    next = reader.Read<T>();

                    if (canModifyValues)
                    {
                        // Integrity validation.
                        if (collection.Count == collectionCountAfterChange)
                        {
                            collection.Remove(next);
                            collection.Add(next);
                        }
                    }
                }

                if (newChangeId)
                    InvokeOnChange(operation, next, false);
            }

            // If changes were made invoke complete after all have been read.
            if (newChangeId && changes > 0)
                InvokeOnChange(SyncHashSetOperation.Complete, default, false);
        }

        /// <summary>
        /// Invokes OnChanged callback.
        /// </summary>
        private void InvokeOnChange(SyncHashSetOperation operation, T item, bool asServer)
        {
            if (asServer)
            {
                if (NetworkBehaviour.OnStartServerCalled)
                    OnChange?.Invoke(operation, item, asServer);
                else
                    _serverOnChanges.Add(new(operation, item));
            }
            else
            {
                if (NetworkBehaviour.OnStartClientCalled)
                    OnChange?.Invoke(operation, item, asServer);
                else
                    _clientOnChanges.Add(new(operation, item));
            }
        }

        /// <summary>
        /// Resets to initialized values.
        /// </summary>
        protected internal override void ResetState(bool asServer)
        {
            base.ResetState(asServer);

            if (CanReset(asServer))
            {
                _sendAll = false;
                _changed.Clear();
                Collection.Clear();

                foreach (T item in _initialValues)
                    Collection.Add(item);
            }
        }

        /// <summary>
        /// Adds value.
        /// </summary>
        /// <param name = "item"></param>
        public bool Add(T item)
        {
            return Add(item, true);
        }

        private bool Add(T item, bool asServer)
        {
            if (!CanNetworkSetValues(true))
                return false;

            bool result = Collection.Add(item);
            // Only process if add was successful.
            if (result && asServer)
                AddOperation(SyncHashSetOperation.Add, item, Collection.Count);

            return result;
        }

        /// <summary>
        /// Adds a range of values.
        /// </summary>
        /// <param name = "range"></param>
        public void AddRange(IEnumerable<T> range)
        {
            foreach (T entry in range)
                Add(entry, true);
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
                AddOperation(SyncHashSetOperation.Clear, default, Collection.Count);
        }

        /// <summary>
        /// Returns if value exist.
        /// </summary>
        /// <param name = "item"></param>
        /// <returns></returns>
        public bool Contains(T item)
        {
            return Collection.Contains(item);
        }

        /// <summary>
        /// Removes a value.
        /// </summary>
        /// <param name = "item"></param>
        /// <returns></returns>
        public bool Remove(T item)
        {
            return Remove(item, true);
        }

        private bool Remove(T item, bool asServer)
        {
            if (!CanNetworkSetValues(true))
                return false;

            bool result = Collection.Remove(item);
            // Only process if remove was successful.
            if (result && asServer)
                AddOperation(SyncHashSetOperation.Remove, item, Collection.Count);

            return result;
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
        /// Looks up obj in Collection and if found marks it's index as dirty.
        /// This operation can be very expensive, will cause allocations, and may fail if your value cannot be compared.
        /// </summary>
        /// <param name = "obj">Object to lookup.</param>
        public void Dirty(T obj)
        {
            if (!IsInitialized)
                return;
            if (!CanNetworkSetValues(true))
                return;

            foreach (T item in Collection)
            {
                if (item.Equals(obj))
                {
                    AddOperation(SyncHashSetOperation.Set, obj, Collection.Count);
                    return;
                }
            }

            // Not found.
            NetworkManager.LogError($"Could not find object within SyncHashSet, dirty will not be set.");
        }

        /// <summary>
        /// Returns Enumerator for collection.
        /// </summary>
        /// <returns></returns>
        public IEnumerator GetEnumerator() => Collection.GetEnumerator();

        [APIExclude]
        IEnumerator<T> IEnumerable<T>.GetEnumerator() => Collection.GetEnumerator();

        [APIExclude]
        IEnumerator IEnumerable.GetEnumerator() => Collection.GetEnumerator();

        public void ExceptWith(IEnumerable<T> other)
        {
            // Again, removing from self is a clear.
            if (other == Collection)
            {
                Clear();
            }
            else
            {
                foreach (T item in other)
                    Remove(item);
            }
        }

        public void IntersectWith(IEnumerable<T> other)
        {
            ISet<T> set;
            if (other is ISet<T> setA)
                set = setA;
            else
                set = new HashSet<T>(other);

            IntersectWith(set);
        }

        private void IntersectWith(ISet<T> other)
        {
            _cache.AddRange(Collection);

            int count = _cache.Count;
            for (int i = 0; i < count; i++)
            {
                T entry = _cache[i];
                if (!other.Contains(entry))
                    Remove(entry);
            }

            _cache.Clear();
        }

        public bool IsProperSubsetOf(IEnumerable<T> other)
        {
            return Collection.IsProperSubsetOf(other);
        }

        public bool IsProperSupersetOf(IEnumerable<T> other)
        {
            return Collection.IsProperSupersetOf(other);
        }

        public bool IsSubsetOf(IEnumerable<T> other)
        {
            return Collection.IsSubsetOf(other);
        }

        public bool IsSupersetOf(IEnumerable<T> other)
        {
            return Collection.IsSupersetOf(other);
        }

        public bool Overlaps(IEnumerable<T> other)
        {
            bool result = Collection.Overlaps(other);
            return result;
        }

        public bool SetEquals(IEnumerable<T> other)
        {
            return Collection.SetEquals(other);
        }

        public void SymmetricExceptWith(IEnumerable<T> other)
        {
            // If calling except on self then that is the same as a clear.
            if (other == Collection)
            {
                Clear();
            }
            else
            {
                foreach (T item in other)
                    Remove(item);
            }
        }

        public void UnionWith(IEnumerable<T> other)
        {
            if (other == Collection)
                return;

            foreach (T item in other)
                Add(item);
        }

        /// <summary>
        /// Adds an item.
        /// </summary>
        /// <param name = "item"></param>
        void ICollection<T>.Add(T item)
        {
            Add(item, true);
        }

        /// <summary>
        /// Copies values to an array.
        /// </summary>
        /// <param name = "array"></param>
        /// <param name = "index"></param>
        public void CopyTo(T[] array, int index)
        {
            Collection.CopyTo(array, index);
        }
    }
}