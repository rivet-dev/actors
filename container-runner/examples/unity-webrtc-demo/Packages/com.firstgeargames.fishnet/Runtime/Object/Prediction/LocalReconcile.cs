using FishNet.Documenting;
using FishNet.Serializing;
using UnityEngine;

namespace FishNet.Object.Prediction
{
    /// <summary>
    /// Used to store reconciles locally.
    /// </summary>
    /// <remarks>This is for internal use only.</remarks>
    [APIExclude]
    public struct LocalReconcile<T> where T : IReconcileData
    {
        /// <summary>
        /// Tick for reconcile.
        /// </summary>
        public uint Tick;
        /// <summary>
        /// Writer reconcile was written to.
        /// </summary>
        public PooledWriter Writer;
        /// <summary>
        /// Data inside writer.
        /// </summary>
        public T Data;

        public void Initialize(uint tick, T data)
        {
            Tick = tick;
            Data = data;
            Writer = WriterPool.Retrieve();
            Writer.Write(data);
        }

        /// <summary>
        /// Disposes of used data.
        /// </summary>
        public void Dispose()
        {
            Data.Dispose();
            if (Writer != null)
                WriterPool.Store(Writer);
        }
    }
}