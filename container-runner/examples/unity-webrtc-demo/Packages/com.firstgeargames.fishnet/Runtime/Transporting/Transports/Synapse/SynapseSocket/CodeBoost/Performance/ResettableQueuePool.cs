using System.Collections.Generic;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for a Queue which is resettable.
    /// </summary>
    public static class ResettableQueuePool<T0> where T0 : IPoolResettable, new()
    {
        /// <summary>
        /// Retrieves an instance of Queue.
        /// </summary>
        public static Queue<T0> Rent() => QueuePool<T0>.Rent();

        /// <summary>
        /// Stores an instance of Queue and sets the original reference to null.
        /// </summary>
        public static void ReturnAndNullifyReference(ref Queue<T0>? value, PoolReturnType collectionReturnType)
        {
            Return(value, collectionReturnType);

            value = null;
        }

        /// <summary>
        /// Stores an instance of Queue.
        /// </summary>
        public static void Return(Queue<T0>? value, PoolReturnType collectionReturnType)
        {
            if (value is null)
                return;

            foreach (T0 item in value)
                item?.OnReturn();

            if (collectionReturnType is PoolReturnType.Return)
                QueuePool<T0>.Return(value);
        }
    }
}
