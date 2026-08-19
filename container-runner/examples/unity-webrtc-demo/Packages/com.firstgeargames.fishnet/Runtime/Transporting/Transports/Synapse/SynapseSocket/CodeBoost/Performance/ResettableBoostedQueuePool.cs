using CodeBoost.Types;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for a BoostedQueue which is resettable.
    /// </summary>
    public static class ResettableBoostedQueuePool<T0> where T0 : IPoolResettable, new()
    {
        /// <summary>
        /// Retrieves an instance of BoostedQueue.
        /// </summary>
        public static BoostedQueue<T0> Rent() => BoostedQueuePool<T0>.Rent();

        /// <summary>
        /// Stores an instance of BoostedQueue and sets the original reference to null.
        /// </summary>
        public static void ReturnAndNullifyReference(ref BoostedQueue<T0>? value, PoolReturnType collectionReturnType)
        {
            Return(value, collectionReturnType);

            value = null;
        }

        /// <summary>
        /// Stores an instance of BoostedQueue.
        /// </summary>
        public static void Return(BoostedQueue<T0>? value, PoolReturnType collectionReturnType)
        {
            if (value is null)
                return;

            while (value.TryDequeue(out T0? item))
                item?.OnReturn();

            if (collectionReturnType is PoolReturnType.Return)
                BoostedQueuePool<T0>.Return(value);
        }
    }
}
