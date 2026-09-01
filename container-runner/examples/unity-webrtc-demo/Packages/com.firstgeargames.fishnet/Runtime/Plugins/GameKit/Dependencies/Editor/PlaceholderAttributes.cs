using UnityEngine;

namespace Sirenix.OdinInspector
{
#if !ODIN_INSPECTOR

    public class TabGroupAttribute : PropertyAttribute
    {
        public string name;
        public bool foldEverything;

        public TabGroupAttribute(string name, bool foldEverything = false)
        {
            this.foldEverything = foldEverything;
            this.name = name;
        }
    }

    public class ShowIfAttribute : PropertyAttribute
    {
        #region Fields
        public string comparedPropertyName { get; private set; }
        public object comparedValue { get; private set; }
        public DisablingType disablingType { get; private set; }

        public enum DisablingType
        {
            ReadOnly = 2,
            DontDraw = 3
        }
        #endregion

        public ShowIfAttribute(string comparedPropertyName, object comparedValue, DisablingType disablingType = DisablingType.DontDraw)
        {
            this.comparedPropertyName = comparedPropertyName;
            this.comparedValue = comparedValue;
            this.disablingType = disablingType;
        }
    }

#endif
}