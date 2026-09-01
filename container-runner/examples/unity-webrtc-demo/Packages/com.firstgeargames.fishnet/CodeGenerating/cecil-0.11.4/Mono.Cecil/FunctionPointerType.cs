//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Collections.Generic;
using System;
using System.Text;
using MD = MonoFN.Cecil.Metadata;

namespace MonoFN.Cecil
{
    public sealed class FunctionPointerType : TypeSpecification, IMethodSignature
    {
        private readonly MethodReference function;
        public bool HasThis
        {
            get { return function.HasThis; }
            set { function.HasThis = value; }
        }
        public bool ExplicitThis
        {
            get { return function.ExplicitThis; }
            set { function.ExplicitThis = value; }
        }
        public MethodCallingConvention CallingConvention
        {
            get { return function.CallingConvention; }
            set { function.CallingConvention = value; }
        }
        public bool HasParameters
        {
            get { return function.HasParameters; }
        }
        public Collection<ParameterDefinition> Parameters
        {
            get { return function.Parameters; }
        }
        public TypeReference ReturnType
        {
            get { return function.MethodReturnType.ReturnType; }
            set { function.MethodReturnType.ReturnType = value; }
        }
        public MethodReturnType MethodReturnType
        {
            get { return function.MethodReturnType; }
        }
        public override string Name
        {
            get { return function.Name; }
            set { throw new InvalidOperationException(); }
        }
        public override string Namespace
        {
            get { return string.Empty; }
            set { throw new InvalidOperationException(); }
        }
        public override ModuleDefinition Module
        {
            get { return ReturnType.Module; }
        }
        public override IMetadataScope Scope
        {
            get { return function.ReturnType.Scope; }
            set { throw new InvalidOperationException(); }
        }
        public override bool IsFunctionPointer
        {
            get { return true; }
        }
        public override bool ContainsGenericParameter
        {
            get { return function.ContainsGenericParameter; }
        }
        public override string FullName
        {
            get
            {
                StringBuilder signature = new();
                signature.Append(function.Name);
                signature.Append(" ");
                signature.Append(function.ReturnType.FullName);
                signature.Append(" *");
                this.MethodSignatureFullName(signature);
                return signature.ToString();
            }
        }

        public FunctionPointerType() : base(null)
        {
            function = new();
            function.Name = "method";
            etype = MD.ElementType.FnPtr;
        }

        public override TypeDefinition Resolve()
        {
            return null;
        }

        public override TypeReference GetElementType()
        {
            return this;
        }
    }
}