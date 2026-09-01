using FishNet.Utility;
using System.Runtime.CompilerServices;
using FishNet.CodeGenerating;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;

[assembly: InternalsVisibleTo(UtilityConstants.CODEGEN_ASSEMBLY_NAME)]

namespace FishNet.Object.Prediction
{
    [MakePublic]
    internal struct ReplicateDataContainer<T> where T : IReplicateData, new()
    {
        #region Types
        private enum DataCachingType
        {
            Unset,
            ValueType,
            IResettableReferenceType,
            ReferenceType
        }
        #endregion

        /// <summary>
        /// Replicate data.
        /// </summary>
        public T Data;
        /// <summary>
        /// True if the data was created locally or came through the network as created.
        /// </summary>
        public bool IsCreated;
        /// <summary>
        /// Channel the data came in on.
        /// </summary>
        public readonly Channel Channel;
        /// <summary>
        /// True if populated.
        /// </summary>
        public bool IsValid { get; private set; }
        /// <summary>
        /// How data should be cached and retrieved when not set.
        /// </summary>
        private static DataCachingType _dataCachingType = DataCachingType.Unset;
        public ReplicateDataContainer(T data, Channel channel) : this(data, channel, tick: 0, isCreated: false) { }
        public ReplicateDataContainer(T data, Channel channel, bool isCreated) : this(data, channel, tick: 0, isCreated) { }

        public ReplicateDataContainer(T data, Channel channel, uint tick, bool isCreated = false)
        {
            Data = data;
            Channel = channel;
            IsCreated = isCreated;
            IsValid = true;

            SetDataTick(tick);
        }

        /// <summary>
        /// A shortcut to calling Data.SetTick.
        /// </summary>
        public void SetDataTick(uint tick)
        {
            SetDataIfNull(ref Data);
            Data.SetTick(tick);
        }

        /// <summary>
        /// Sets data to new() if is nullable type, and is null.
        /// </summary>
        /// <param name = "data"></param>
        private void SetDataIfNull(ref T data)
        {
            // Only figure out data caching type once to save perf.
            if (_dataCachingType == DataCachingType.Unset)
            {
                if (typeof(T).IsValueType)
                    _dataCachingType = DataCachingType.ValueType;
                else if (typeof(IResettable).IsAssignableFrom(typeof(T)))
                    _dataCachingType = DataCachingType.IResettableReferenceType;
                else
                    _dataCachingType = DataCachingType.ReferenceType;
            }

            if (_dataCachingType != DataCachingType.ValueType && data == null)
                data = ObjectCaches<T>.Retrieve();
        }

        public void Dispose()
        {
            if (Data != null)
                Data.Dispose();

            IsValid = false;
        }

        public static ReplicateDataContainer<T> GetDefault(uint tick) => new(default, Channel.Unreliable, tick);
        public static ReplicateDataContainer<T> GetDefault() => GetDefault(tick: 0);
    }
}