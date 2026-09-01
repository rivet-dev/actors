using System.Collections.Generic;

namespace GameKit.Dependencies.Utilities
{
    public static class ListsFN
    {

        /// <summary>
        /// Adds items to collection while preventing duplicates, returning number of items added.
        /// </summary>
        public static int AddRangeUnique<T>(this List<T> collection, IEnumerable<T> items)
        {
            int added = 0;
            
            foreach (T item in items) 
            {
                if (!collection.Contains(item))
                {
                    collection.Add(item);
                    added++;
                }
            }
            
            return added;
        }

        
        /// <summary>
        /// Adds item to collection while preventing duplicates, returning if added.
        /// </summary>
        public static bool AddUnique<T>(this List<T> collection, T item)
        {
            if (!collection.Contains(item))
            {
                collection.Add(item);
                return true;
            }

            return false;
        }


     
    }
}