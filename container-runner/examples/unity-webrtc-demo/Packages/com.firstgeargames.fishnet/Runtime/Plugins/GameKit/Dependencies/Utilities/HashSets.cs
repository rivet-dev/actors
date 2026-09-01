using System.Collections.Generic;

namespace GameKit.Dependencies.Utilities
{
    public static class HashSetsFN
    {

        /// <summary>
        /// Adds a collection of items.
        /// </summary>
        public static void AddRange<T>(this HashSet<T> hashSet, IEnumerable<T> items) 
        {
            foreach (T item in items)
                hashSet.Add(item);
        }

        /// <summary>
        /// Returns values as a list.
        /// </summary>
        /// <returns></returns>
        public static List<T> ToList<T>(this HashSet<T> collection, bool useCache)
        {
            List<T> result = useCache ? CollectionCaches<T>.RetrieveList() : new(collection.Count);
            
            //No need to clear the list since it's already clear.
            collection.ToList(ref result, clearLst: false);
            
            return result;
        }

        /// <summary>
        /// Adds values to a list.
        /// </summary>
        /// <returns></returns>
        public static void ToList<T>(this HashSet<T> collection, ref List<T> lst, bool clearLst)
        {
            if (clearLst)
                lst.Clear();
            
            foreach (T item in collection)
                lst.Add(item);
        }
    }
}