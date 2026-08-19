using System;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types
{
    /// <summary>
    /// Moves smoothly to transform changes over ticks giving cameras something to follow.
    /// </summary>
    public class SmoothCameraTarget : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// Created instance of DDOL.
        /// </summary>
        private static DDOL _instance;
        #endregion

        /// <summary>
        /// Returns the current DDOL or creates one if not yet created.
        /// </summary>
        public static DDOL GetDDOL()
        {
            // Not yet made.
            if (_instance == null)
            {
                GameObject obj = new();
                obj.name = "FirstGearGames DDOL";
                DDOL ddol = obj.AddComponent<DDOL>();
                DontDestroyOnLoad(ddol);
                _instance = ddol;
                return ddol;
            }
            // Already  made.
            else
            {
                return _instance;
            }
        }
    }
}