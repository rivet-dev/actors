using System.Collections.Generic;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for a Dictionary which is resettable.
    /// </summary>
    public static class ResettableT0SortedListPool<T0, T1> where T0 : IPoolResettable, new()
    {
        /// <summary>
        /// Retrieves an instance of Dictionary.
        /// </summary>
        public static SortedList<T0, T1> Rent() => SortedListPool<T0, T1>.Rent();

        /// <summary>
        /// Stores an instance of Dictionary and sets the original reference to null.
        /// </summary>
        public static void ReturnAndNullifyReference(ref SortedList<T0, T1>? value, PoolReturnType collectionReturnType)
        {
            Return(value, collectionReturnType);

            value = null;
        }

        /// <summary>
        /// Stores an instance of Dictionary.
        /// </summary>
        public static void Return(SortedList<T0, T1>? value, PoolReturnType collectionReturnType)
        {
            if (value is null)
                return;

            foreach (T0 item in value.Keys)
                item?.OnReturn();

            if (collectionReturnType is PoolReturnType.Return)
                SortedListPool<T0, T1>.Return(value);
        }
    }
}
