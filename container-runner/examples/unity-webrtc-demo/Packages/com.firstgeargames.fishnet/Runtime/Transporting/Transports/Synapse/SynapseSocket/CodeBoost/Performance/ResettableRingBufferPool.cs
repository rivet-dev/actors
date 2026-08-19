using CodeBoost.Types;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for a RingBuffer which is resettable.
    /// </summary>
    public static class ResettableRingBufferPool<T0> where T0 : IPoolResettable, new()
    {
        /// <summary>
        /// Retrieves an instance of RingBuffer.
        /// </summary>
        public static RingBuffer<T0> Rent() => RingBufferPool<T0>.Rent();

        /// <summary>
        /// Stores an instance of RingBuffer and sets the original reference to null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref RingBuffer<T0>? value, PoolReturnType collectionReturnType)
        {
            Return(value, collectionReturnType);

            value = null;
        }

        /// <summary>
        /// Stores an instance of RingBuffer.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void Return(RingBuffer<T0>? value, PoolReturnType collectionReturnType)
        {
            if (value is null)
                return;

            foreach (T0 item in value)
                item?.OnReturn();

            if (collectionReturnType is PoolReturnType.Return)
                RingBufferPool<T0>.Return(value);
        }
    }
}
