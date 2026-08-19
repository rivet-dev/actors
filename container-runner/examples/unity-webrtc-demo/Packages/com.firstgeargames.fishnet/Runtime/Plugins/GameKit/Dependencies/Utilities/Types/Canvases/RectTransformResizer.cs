using System.Collections.Generic;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types
{
    /// <summary>
    /// Gameplay canvases register to this manager.
    /// </summary>
    public class RectTransformResizer : MonoBehaviour
    {
        #region Types.
        public class ResizeData : IResettable
        {
            public byte Remaining;
            public ResizeDelegate Delegate;

            public ResizeData()
            {
                Remaining = 2;
            }

            public void InitializeState() { }

            public void ResetState()
            {
                Remaining = 2;
                Delegate = null;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Delegate for resizing RectTransforms.
        /// </summary>
        /// <param name = "complete">True if the resize iterations are complete. Typically show your visuals when true.</param>
        public delegate void ResizeDelegate(bool complete);
        #endregion

        #region Private.
        /// <summary>
        /// Elements to resize.
        /// </summary>
        private List<ResizeData> _resizeDatas = new();
        /// <summary>
        /// Singleton instance of this class.
        /// </summary>
        private static RectTransformResizer _instance;
        #endregion

        private void OnDestroy()
        {
            foreach (ResizeData item in _resizeDatas)
                ResettableObjectCaches<ResizeData>.Store(item);
        }

        private void Update()
        {
            Resize();
        }

        /// <summary>
        /// Calls pending resizeDatas.
        /// </summary>
        private void Resize()
        {
            for (int i = 0; i < _resizeDatas.Count; i++)
            {
                _resizeDatas[i].Remaining--;
                bool complete = _resizeDatas[i].Remaining == 0;
                _resizeDatas[i].Delegate?.Invoke(complete);
                if (complete)
                {
                    ResettableObjectCaches<ResizeData>.Store(_resizeDatas[i]);
                    _resizeDatas.RemoveAt(i);
                    i--;
                }
            }
        }

        /// <summary>
        /// Used to call a delegate twice, over two frames.
        /// This is an easy way to resize RectTransforms multiple times as they will often fail after the first resize due to Unity limitations.
        /// Note: this work-around may not be required for newer Unity versions.
        /// </summary>
        /// <param name = "del">Delegate to invoke when resizing completes.</param>
        public static void Resize(ResizeDelegate del)
        {
            // Check to make a singleton instance.
            if (_instance == null)
            {
                GameObject go = new(typeof(RectTransformResizer).Name);
                _instance = go.AddComponent<RectTransformResizer>();
                DontDestroyOnLoad(go);
            }

            _instance.Resize_Internal(del);
        }

        private void Resize_Internal(ResizeDelegate del)
        {
            ResizeData rd = ResettableObjectCaches<ResizeData>.Retrieve();
            rd.Delegate = del;
            _instance._resizeDatas.Add(rd);
        }
    }
}