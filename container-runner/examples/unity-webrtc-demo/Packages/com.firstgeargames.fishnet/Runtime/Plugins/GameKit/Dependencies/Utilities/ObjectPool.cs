// /* This implementation uses better naming as well provides
//  * a few new features.
//  *
//  * Object and Collection caches are now one class. */
//
// // TODO In V5 disappear ResettableRingBuffer and have regular check implementation on start -- this will let us use this class for caching ringbuffer with resettable types.
// using System;
// using System.Collections;
// using System.Collections.Generic;
// using System.Runtime.CompilerServices;
//
// namespace GameKit.Dependencies.Utilities
// {
//     /// <summary>
//     /// Implement to use type with Caches.
//     /// </summary>
//     public interface IResettable
//     {
//         /// <summary>
//         /// Resets values when being placed in a cache.
//         /// </summary>
//         void ResetState();
//     
//         /// <summary>
//         /// Initializes values after being retrieved from a cache.
//         /// </summary>
//         void InitializeState();
//     }
//
//     public static class ResettableObjectPool<T> where T : IResettable, new() { }
//
//     /// <summary>
//     /// Caches collections and objects of T.
//     /// </summary>
//     public static class ObjectPool<T> where T : new()
//     {
//         /// <summary>
//         /// Cache for List<T>.
//         /// </summary>
//         private static readonly Stack<List<T>> _listCache = new();
//         /// <summary>
//         /// Resettable cache for List<T>.
//         /// </summary>
//         private static readonly Stack<List<T>> _resettableListCache = new();
//
//         /// <summary>
//         /// Cache for HashSet<T>.
//         /// </summary>
//         private static readonly Stack<HashSet<T>> _hashSetCache = new();
//         /// <summary>
//         /// Resettable cache for HashSet<T>.
//         /// </summary>
//         private static readonly Stack<HashSet<T>> _resettableHashSetCache = new();
//
//         /// <summary>
//         /// Cache for Queue<T>.
//         /// </summary>
//         private static readonly Stack<Queue<T>> _queueCache = new();
//         /// <summary>
//         /// Resettable cache for Queue<T>.
//         /// </summary>
//         private static readonly Stack<Queue<T>> _resettableQueueCache = new();
//
//         /// <summary>
//         /// Cache for T[].
//         /// </summary>
//         private static readonly Stack<T[]> _arrayCache = new();
//         /// <summary>
//         /// Resettable cache for T[].
//         /// </summary>
//         private static readonly Stack<T[]> _resettableArrayCache = new();
//
//         /// <summary>
//         /// Cache for T.
//         /// </summary>
//         private static readonly Stack<T> _tCache = new();
//         /// <summary>
//         /// Resettable cache for T.
//         /// </summary>
//         private static readonly Stack<T> _resettableTCache = new();
//
//         /// <summary>
//         /// True if T is a value type.
//         /// </summary>
//         private static readonly bool _isValueType;
//         /// <summary>
//         /// True if T implements IResettable.
//         /// </summary>
//         private static readonly bool _isResettable;
//
//         static ObjectPool()
//         {
//             // Used at runtime to prevent nested collections.
//             bool isTCollection = typeof(ICollection).IsAssignableFrom(typeof(T));
//
//             if (isTCollection)
//                 throw new NotSupportedException($"ObjectPool element cannot be a collection. Type is [{typeof(T).FullName}].");
//
//             _isValueType = typeof(T).IsValueType;
//             _isResettable = typeof(T).IsAssignableFrom(typeof(IResettable));
//         }
//
//         /// <summary>
//         /// Clears all pools for T.
//         /// </summary>
//         public static void ClearPools()
//         {
//             _listCache.Clear();
//             _resettableListCache.Clear();
//
//             _hashSetCache.Clear();
//             _resettableHashSetCache.Clear();
//
//             _queueCache.Clear();
//             _resettableQueueCache.Clear();
//
//             _arrayCache.Clear();
//             _resettableArrayCache.Clear();
//
//             _tCache.Clear();
//             _resettableTCache.Clear();
//         }
//
//         /// <summary>
//         /// Returns a List<T> automatically resetting entries when IResettable is implemented,
//         /// and pooling entries when they are a reference type.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         public static void Return(List<T> value)
//         {
//             if (value == null) return;
//
//             Stack<List<T>> stack = _isResettable ? _resettableListCache : _listCache;
//
//             IterateICollectionElements(value);
//
//             value.Clear();
//             stack.Push(value);
//         }
//
//         /// <summary>
//         /// Returns a HashSet<T> automatically resetting entries when IResettable is implemented,
//         /// and pooling entries when they are a reference type.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         public static void Return(HashSet<T> value)
//         {
//             if (value == null) return;
//
//             Stack<HashSet<T>> stack = _isResettable ? _resettableHashSetCache : _hashSetCache;
//
//             IterateICollectionElements(value);
//
//             value.Clear();
//             stack.Push(value);
//         }
//
//         /// <summary>
//         /// Returns a Queue<T> automatically resetting entries when IResettable is implemented,
//         /// and pooling entries when they are a reference type.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         public static void Return(Queue<T> value)
//         {
//             if (value == null) return;
//
//             Stack<Queue<T>> stack = _isResettable ? _resettableQueueCache : _queueCache;
//
//             IterateICollectionElements(value);
//
//             value.Clear();
//             stack.Push(value);
//         }
//
//         /// <summary>
//         /// Returns an array automatically resetting entries when IResettable is implemented,
//         /// and pools each array entry if the array element is a reference type.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         public static void Return(T[] value)
//         {
//             if (value == null) return;
//
//             Stack<T[]> stack = _isResettable ? _resettableArrayCache : _arrayCache;
//
//             IterateICollectionElements(value);
//
//             Array.Clear(value, 0, value.Length);
//             stack.Push(value);
//         }
//
//         /// <summary>
//         /// Returns value without resetting the state.
//         /// </summary>
//         public static void Return(T value)
//         {
//             _tCache.Push(value);
//         }
//
//         /// <summary>
//         /// Creates a new List<T>.
//         /// </summary>
//         private static List<T> CreateList() => new();
//
//         /// <summary>
//         /// Creates a new HashSet<T>.
//         /// </summary>
//         private static HashSet<T> CreateHashSet() => new();
//
//         /// <summary>
//         /// Creates a new Queue<T>.
//         /// </summary>
//         private static Queue<T> CreateQueue() => new();
//
//         /// <summary>
//         /// Creates a new array of length 0 (empty array).
//         /// </summary>
//         private static T[] CreateArray() => Array.Empty<T>();
//
//         /// <summary>
//         /// Rents a List<T>.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         public static List<T> RentList()
//         {
//             Stack<List<T>> stack = _isResettable ? _resettableListCache : _listCache;
//
//             return RentCollection(stack, CreateList);
//         }
//
//         /// <summary>
//         /// Rents a HashSet<T>.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         public static HashSet<T> RentHashSet()
//         {
//             Stack<HashSet<T>> stack = _isResettable ? _resettableHashSetCache : _hashSetCache;
//
//             return RentCollection(stack, CreateHashSet);
//         }
//
//         /// <summary>
//         /// Rents a Queue<T>.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         public static Queue<T> RentQueue()
//         {
//             Stack<Queue<T>> stack = _isResettable ? _resettableQueueCache : _queueCache;
//
//             return RentCollection(stack, CreateQueue);
//         }
//
//         /// <summary>
//         /// Rents an array.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         public static T[] RentArray()
//         {
//             Stack<T[]> stack = _isResettable ? _resettableArrayCache : _arrayCache;
//
//             return RentCollection(stack, CreateArray);
//         }
//
//         /// <summary>
//         /// Rents an object.
//         /// </summary>
//         public static T Rent()
//         {
//             Stack<T> stack = _isResettable ? _resettableTCache : _tCache;
//
//             if (!stack.TryPop(out T result))
//                 result = new();
//
//             return result;
//         }
//
//         /// <summary>
//         /// Rents a collection using the supplied stack. Returns using defaultFactory if stack is empty.
//         /// </summary>
//         private static TCollection RentCollection<TCollection>(Stack<TCollection> stack, Func<TCollection> defaultFactory)
//         {
//             if (!stack.TryPop(out TCollection result))
//                 result = defaultFactory();
//
//             return result;
//         }
//
//         /// <summary>
//         /// Iterates ICollection elements, returning and resetting as needed.
//         /// </summary>
//         [MethodImpl(MethodImplOptions.AggressiveInlining)]
//         private static void IterateICollectionElements(IReadOnlyCollection<T> value)
//         {
//             // Reset T if possible.
//             if (_isResettable)
//             {
//                 // Value type.
//                 if (_isValueType)
//                 {
//                     foreach (T item in value)
//                         ((IResettable)item).ResetState();
//                 }
//                 // Reference type.
//                 else
//                 {
//                     foreach (T item in value)
//                         ReturnReferenceIResettable(item);
//                 }
//             }
//             // Type is not resettable.
//             else
//             {
//                 // Only need to Return if is not a value type.
//                 if (!_isValueType)
//                 {
//                     foreach (T item in value)
//                         ReturnReference(item);
//                 }
//             }
//         }
//
//         /// <summary>
//         /// Returns value expecting it to be a reference type that does not implement IResettable.
//         /// </summary>
//         private static void ReturnReference(T value)
//         {
//             _resettableTCache.Push(value);
//         }
//
//         /// <summary>
//         /// Returns value expecting it to be a reference type which implement IResettable.
//         /// </summary>
//         internal static void ReturnReferenceIResettable(T value)
//         {
//             ((IResettable)value).ResetState();
//
//             _resettableTCache.Push(value);
//         }
//     }
// }

