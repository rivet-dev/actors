using FishNet.Object;
using System;
using UnityEngine;

public class NetworkPlayerController : NetworkBehaviour
{
    [SerializeField]
    private float _moveSpeed = 5f;

    [SerializeField]
    private float _turnSpeed = 720f;

    [SerializeField]
    private Vector3 _cameraOffset = new(0f, 6f, -8f);

    private Camera _mainCamera;
    private bool _autoMove;
    private float _autoMoveTime;

    public override void OnStartClient()
    {
        base.OnStartClient();

        if (IsOwner)
        {
            _mainCamera = Camera.main;
            _autoMove = ShouldAutoMove();
            if (_autoMove)
                ConfigureAutoMoveCamera();
        }
    }

    private void Update()
    {
        if (!IsOwner)
            return;

        Vector3 input = _autoMove ? GetAutoMoveInput() : GetMoveInput();
        if (input.sqrMagnitude <= 0f)
            return;

        transform.position += input * (_moveSpeed * Time.deltaTime);

        Quaternion targetRotation = Quaternion.LookRotation(input, Vector3.up);
        transform.rotation = Quaternion.RotateTowards(
            transform.rotation,
            targetRotation,
            _turnSpeed * Time.deltaTime);
    }

    private void LateUpdate()
    {
        if (!IsOwner)
            return;
        if (_autoMove)
            return;

        if (_mainCamera == null)
            _mainCamera = Camera.main;
        if (_mainCamera == null)
            return;

        _mainCamera.transform.position = transform.position + _cameraOffset;
        _mainCamera.transform.LookAt(transform.position + Vector3.up);
    }

    private static Vector3 GetMoveInput()
    {
        float x = 0f;
        float z = 0f;

        if (Input.GetKey(KeyCode.A) || Input.GetKey(KeyCode.LeftArrow))
            x -= 1f;
        if (Input.GetKey(KeyCode.D) || Input.GetKey(KeyCode.RightArrow))
            x += 1f;
        if (Input.GetKey(KeyCode.W) || Input.GetKey(KeyCode.UpArrow))
            z += 1f;
        if (Input.GetKey(KeyCode.S) || Input.GetKey(KeyCode.DownArrow))
            z -= 1f;

        Vector3 input = new(x, 0f, z);
        return input.sqrMagnitude > 1f ? input.normalized : input;
    }

    private Vector3 GetAutoMoveInput()
    {
        _autoMoveTime += Time.deltaTime;
        float phase = _autoMoveTime % 6f;

        if (phase < 1.5f)
            return Vector3.forward * 0.35f;
        if (phase < 3f)
            return Vector3.right * 0.35f;
        if (phase < 4.5f)
            return Vector3.back * 0.35f;
        return Vector3.left * 0.35f;
    }

    private void ConfigureAutoMoveCamera()
    {
        if (_mainCamera == null)
            return;

        _mainCamera.orthographic = true;
        _mainCamera.orthographicSize = 8f;
        _mainCamera.transform.position = new Vector3(0f, 16f, 0f);
        _mainCamera.transform.rotation = Quaternion.Euler(90f, 0f, 0f);
    }

    private static bool ShouldAutoMove()
    {
        foreach (string arg in Environment.GetCommandLineArgs())
        {
            if (arg == "-automove")
                return true;
        }

        return Environment.GetEnvironmentVariable("FISHNET_AUTOMOVE") == "1";
    }
}
