#if NEW_INPUTSYSTEM
using UnityEngine;
using UnityEngine.InputSystem;

namespace GameKit.Dependencies.Utilities
{
    public static class NewInput
    {
        /// <summary>
        /// Current Keyboard.
        /// </summary>
        public static Keyboard Keyboard => Keyboard.current;
        /// <summary>
        /// Current Mouse.
        /// </summary>
        public static Mouse Mouse => Mouse.current;

        /// <summary>
        /// Returns if a button is held on any map.
        /// </summary>
        public static bool GetButtonHeld(Key key)
        {
            return (Keyboard != null) ? Keyboard[key].isPressed : false;
        }

        /// <summary>
        /// Returns if a button is pressed on any map.
        /// </summary>
        public static bool GetButtonPressed(Key key)
        {
            return (Keyboard != null) ? Keyboard[key].wasPressedThisFrame : false;
        }

        /// <summary>
        /// Returns if a button is released on any map.
        /// </summary>
        public static bool GetButtonReleased(Key key)
        {
            return (Keyboard != null) ? Keyboard[key].wasReleasedThisFrame : false;
        }
    }

    public static class MouseExtensions
    {
        public static Vector3 GetPosition(this Mouse m)
        {
            return m.position.ReadValue();
        }
    }

    public static class KeyboardExtensions
    {
        public static bool GetKeyPressed(this Keyboard kb, Key kc)
        {
            return kb[kc].wasPressedThisFrame;
        }

        public static bool GetKeyHeld(this Keyboard kb, Key kc)
        {
            return kb[kc].isPressed;
        }

        public static bool GetKeyReleased(this Keyboard kb, Key kc)
        {
            return kb[kc].wasReleasedThisFrame;
        }
    }

    public static class InputActionMapExtensions
    {
        #region Strings.
        public static float GetAxisRaw(this InputActionMap map, string negativeName, string positiveName)
        {
            return map.GetAxisRaw(negativeName, positiveName, out _);
        }

        public static float GetAxisRaw(this InputActionMap map, string negativeName, string positiveName, out bool found)
        {
            found = false;
            InputAction negativeIa = map.FindAction(negativeName);
            InputAction positiveIa = map.FindAction(positiveName);
            if (negativeIa == null || positiveIa == null)
                return 0f;

            found = true;
            bool negativePressed = negativeIa.IsPressed();
            bool positivePressed = positiveIa.IsPressed();
            /* If both are pressed then they cancel each other out.
             * And if neither are pressed then result is naturally
             * 0f. */
            if (negativePressed == positivePressed)
                return 0f;
            else if (negativePressed)
                return -1f;
            else
                return 1f;
        }

        public static float GetAxisRaw(this InputActionMap map, string inputName)
        {
            return map.GetAxisRaw(inputName, out _);
        }

        public static float GetAxisRaw(this InputActionMap map, string inputName, out bool found)
        {
            found = false;
            InputAction ia = map.FindAction(inputName);
            if (ia == null)
                return 0f;

            found = true;
            float axis = ia.ReadValue<float>();
            if (axis == 0f)
                return 0f;
            else
                return Mathf.Sign(axis);
        }

        public static bool GetButtonHeld(this InputActionMap map, string inputName)
        {
            InputAction ia = map.FindAction(inputName);
            return (ia == null) ? false : ia.IsPressed();
        }

        public static bool GetButtonPressed(this InputActionMap map, string inputName)
        {
            InputAction ia = map.FindAction(inputName);
            return (ia == null) ? false : ia.WasPressedThisFrame();
        }

        public static bool GetButtonReleased(this InputActionMap map, string inputName)
        {
            InputAction ia = map.FindAction(inputName);
            return (ia == null) ? false : ia.WasReleasedThisFrame();
        }
        #endregion

        #region InputActions.
        public static float GetAxisRaw(InputAction negativeIa, InputAction positiveIa)
        {
            return GetAxisRaw(negativeIa, positiveIa, out _);
        }

        public static float GetAxisRaw(InputAction negativeIa, InputAction positiveIa, out bool found)
        {
            found = false;
            if (negativeIa == null || positiveIa == null)
                return 0f;

            found = true;
            bool negativePressed = negativeIa.IsPressed();
            bool positivePressed = positiveIa.IsPressed();
            /* If both are pressed then they cancel each other out.
             * And if neither are pressed then result is naturally
             * 0f. */
            if (negativePressed == positivePressed)
                return 0f;
            else if (negativePressed)
                return -1f;
            else
                return 1f;
        }

        public static float GetAxisRaw(this InputAction ia)
        {
            return GetAxisRaw(ia, out _);
        }

        public static float GetAxisRaw(this InputAction ia, out bool found)
        {
            found = false;
            if (ia == null)
                return 0f;

            found = true;
            float axis = ia.ReadValue<float>();
            if (axis == 0f)
                return 0f;
            else
                return Mathf.Sign(axis);
        }

        public static bool GetButtonHeld(this InputAction ia)
        {
            return (ia == null) ? false : ia.IsPressed();
        }

        public static bool GetButtonPressed(this InputAction ia)
        {
            return (ia == null) ? false : ia.WasPressedThisFrame();
        }

        public static bool GetButtonReleased(this InputAction ia)
        {
            return (ia == null) ? false : ia.WasReleasedThisFrame();
        }
        #endregion
    }
}
#endif