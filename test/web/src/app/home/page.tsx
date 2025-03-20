"use client"

import Box from '@mui/material/Box';

export default function App() {
    return (
            <Box
                component="form"
                noValidate
                sx={{
                    display: 'flex',
                    flexDirection: 'column',
                    width: '100%',
                    gap: 2,
                }}
            />
    );
}