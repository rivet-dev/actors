using System.Collections.Generic;

namespace GameKit.Dependencies.Utilities.Types
{
    /// <summary>
    /// Used to track generic canvases and their states.
    /// </summary>
    public static class CanvasTracker
    {
        /// <summary>
        /// Canvases which should block input.
        /// </summary>
        public static IReadOnlyList<object> InputBlockingCanvases => _inputBlockingCanvases;
        private static List<object> _inputBlockingCanvases = new();
        /// <summary>
        /// Canvases which are currently open, in the order they were opened.
        /// </summary>
        public static IReadOnlyList<object> OpenCanvases => _openCanvases;
        private static List<object> _openCanvases = new();
        /// <summary>
        /// True if any blocking canvas is open.
        /// </summary>
        public static bool IsInputBlockingCanvasOpen => _inputBlockingCanvases.Count > 0;

        /// <summary>
        /// Returns true if is the last canvas opened or if no canvases are set as opened.
        /// </summary>
        public static bool IsLastOpenCanvas(object canvas) => IsEmptyCollectionOrLastEntry(canvas, _openCanvases);

        /// <summary>
        /// Returns true if is the last canvas blocking input or if no input blocking canvases are set as opened.
        /// </summary>
        public static bool IsLastInputBlockingCanvas(object canvas) => IsEmptyCollectionOrLastEntry(canvas, _inputBlockingCanvases);

        /// <summary>
        /// Returns true if canvas is the last object in collection or collection is empty.
        /// </summary>
        /// <returns></returns>
        private static bool IsEmptyCollectionOrLastEntry(object canvas, List<object> collection)
        {
            int count = collection.Count;
            if (count == 0)
                return true;

            return collection[count - 1] == canvas;
        }

        /// <summary>
        /// Clears all collections.
        /// </summary>
        public static void ClearCollections()
        {
            _openCanvases.Clear();
            _inputBlockingCanvases.Clear();
        }

        /// <summary>
        /// Removes null references of canvases.
        /// This can be used as clean-up if you were unable to remove a canvas properly.
        /// Using this method regularly could be expensive if there are hundreds of open canvases.
        /// </summary>
        public static void RemoveNullReferences()
        {
            RemoveNullEntries(_openCanvases);
            RemoveNullEntries(_inputBlockingCanvases);

            void RemoveNullEntries(List<object> collection)
            {
                for (int i = 0; i < collection.Count; i++)
                {
                    if (collection[i] == null)
                    {
                        collection.RemoveAt(i);
                        i--;
                    }
                }
            }
        }

        /// <summary>
        /// Returns true if canvas is an open canvas.
        /// </summary>
        public static bool IsOpenCanvas(object canvas)
        {
            return _openCanvases.Contains(canvas);
        }

        /// <summary>
        /// Returns if the canvas is an input blocking canvas.
        /// </summary>
        public static bool IsInputBlockingCanvas(object canvas)
        {
            return _inputBlockingCanvases.Contains(canvas);
        }

        /// <summary>
        /// Adds a canvas to OpenCanvases if not already added.
        /// </summary>
        /// <param name = "addToBlocking">True to also add as an input blocking canvas.</param>
        /// <returns>True if the canvas was added, false if already added.</returns>
        public static bool AddOpenCanvas(object canvas, bool addToBlocking)
        {
            bool added = _openCanvases.AddUnique(canvas);
            if (added && addToBlocking)
                _inputBlockingCanvases.Add(canvas);

            return added;
        }

        /// <summary>
        /// Removes a canvas from OpenCanvases.
        /// </summary>
        /// <returns>True if the canvas was removed, false if it was not added.</returns>
        public static bool RemoveOpenCanvas(object canvas)
        {
            _inputBlockingCanvases.Remove(canvas);
            return _openCanvases.Remove(canvas);
        }
    }
}