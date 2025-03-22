"use client";

import { useState, useEffect } from "react";
import {TextField, Box, FormControl, FormLabel, Button, Divider} from "@mui/material";

export default function App() {
    const [sipUri, setSipUri] = useState('');


    useEffect(() => {
        const lastSipUri = localStorage.getItem('last_sip_uri') || '';
        if (lastSipUri) {
            setSipUri(lastSipUri);
        }
    });

    const txtSipUriOnChange = (e) => {
        localStorage.setItem('last_sip_uri', e.target.value);
        setSipUri(e.target.value);
    };

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
            >
                <FormControl>
                    <FormLabel htmlFor="email">sip_uri</FormLabel>
                    <TextField
                        autoFocus={true}
                        value={sipUri}
                        variant="outlined"
                        onChange={(e) => txtSipUriOnChange(e)}/>
                </FormControl>
                <Button
                    type="submit"
                    fullWidth
                    variant="contained"
                >
                    run
                </Button>
                <Button
                    type="submit"
                    fullWidth
                    variant="contained"
                >
                    stop
                </Button>
                <Divider>logs</Divider>
                <TextField
                    label=''
                    multiline
                />
            </Box>
    );
}